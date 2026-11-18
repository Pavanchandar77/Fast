#![cfg(windows)]

use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use crate::error::ShieldError;
use crate::system::{
    RegistryHive, RegistryValue, ServiceStartType, ServiceState, ServiceStatus, SystemBackend,
    TaskState,
};

const CREATE_NO_WINDOW: u32 = 0x08000000;

pub struct WindowsSystem {
    hosts_path: PathBuf,
}

impl Default for WindowsSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowsSystem {
    pub fn new() -> Self {
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
        let hosts_path = PathBuf::from(system_root).join(r"System32\drivers\etc\hosts");
        Self { hosts_path }
    }

    pub fn with_hosts_path(hosts_path: PathBuf) -> Self {
        Self { hosts_path }
    }

    fn run_cmd(cmd: &str, args: &[&str]) -> Result<(bool, String), ShieldError> {
        let mut command = Command::new(cmd);
        command.args(args);
        command.creation_flags(CREATE_NO_WINDOW);

        match command.output() {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let full = format!("{stdout}\n{stderr}");
                Ok((output.status.success(), full))
            }
            Err(e) => Err(ShieldError::Io(format!("Failed to execute {cmd}: {e}"))),
        }
    }
}

impl SystemBackend for WindowsSystem {
    fn get_registry_value(
        &self,
        hive: RegistryHive,
        subkey: &str,
        value_name: &str,
    ) -> Result<Option<RegistryValue>, ShieldError> {
        let key_str = format!("{hive}\\{subkey}");
        let (success, output) = Self::run_cmd("reg.exe", &["query", &key_str, "/v", value_name])?;

        if !success {
            return Ok(None);
        }

        // Parse reg.exe output:
        // ValueName    REG_DWORD    0x0
        // ValueName    REG_SZ       Allow
        for line in output.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 && parts[0].eq_ignore_ascii_case(value_name) {
                let reg_type = parts[1];
                let val_str = parts[2..].join(" ");

                return match reg_type {
                    "REG_DWORD" => {
                        let parsed = if val_str.starts_with("0x") || val_str.starts_with("0X") {
                            u32::from_str_radix(&val_str[2..], 16).unwrap_or(0)
                        } else {
                            val_str.parse::<u32>().unwrap_or(0)
                        };
                        Ok(Some(RegistryValue::Dword(parsed)))
                    }
                    "REG_QWORD" => {
                        let parsed = if val_str.starts_with("0x") || val_str.starts_with("0X") {
                            u64::from_str_radix(&val_str[2..], 16).unwrap_or(0)
                        } else {
                            val_str.parse::<u64>().unwrap_or(0)
                        };
                        Ok(Some(RegistryValue::Qword(parsed)))
                    }
                    "REG_SZ" => Ok(Some(RegistryValue::Sz(val_str))),
                    "REG_EXPAND_SZ" => Ok(Some(RegistryValue::ExpandSz(val_str))),
                    _ => Ok(Some(RegistryValue::Sz(val_str))),
                };
            }
        }

        Ok(None)
    }

    fn set_registry_value(
        &mut self,
        hive: RegistryHive,
        subkey: &str,
        value_name: &str,
        value: RegistryValue,
    ) -> Result<(), ShieldError> {
        let key_str = format!("{hive}\\{subkey}");
        let (type_str, val_str) = match &value {
            RegistryValue::Dword(v) => ("REG_DWORD", v.to_string()),
            RegistryValue::Qword(v) => ("REG_QWORD", v.to_string()),
            RegistryValue::Sz(s) => ("REG_SZ", s.clone()),
            RegistryValue::ExpandSz(s) => ("REG_EXPAND_SZ", s.clone()),
            RegistryValue::MultiSz(list) => ("REG_MULTI_SZ", list.join(r"\0")),
            RegistryValue::Binary(bytes) => {
                let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
                ("REG_BINARY", hex)
            }
        };

        let (success, out) = Self::run_cmd(
            "reg.exe",
            &[
                "add", &key_str, "/v", value_name, "/t", type_str, "/d", &val_str, "/f",
            ],
        )?;

        if !success {
            return Err(ShieldError::Backend(format!(
                "Failed to set registry value {key_str}\\{value_name}: {out}"
            )));
        }

        Ok(())
    }

    fn delete_registry_value(
        &mut self,
        hive: RegistryHive,
        subkey: &str,
        value_name: &str,
    ) -> Result<(), ShieldError> {
        let key_str = format!("{hive}\\{subkey}");
        let (success, out) =
            Self::run_cmd("reg.exe", &["delete", &key_str, "/v", value_name, "/f"])?;

        if !success && !out.contains("unable to find") {
            return Err(ShieldError::Backend(format!(
                "Failed to delete registry value {key_str}\\{value_name}: {out}"
            )));
        }

        Ok(())
    }

    fn get_service_state(&self, service_name: &str) -> Result<Option<ServiceState>, ShieldError> {
        let (success, output) = Self::run_cmd("sc.exe", &["qc", service_name])?;
        if !success {
            return Ok(None);
        }

        let start_type = if output.contains("DEMAND_START") {
            ServiceStartType::Manual
        } else if output.contains("AUTO_START") {
            ServiceStartType::Automatic
        } else if output.contains("DISABLED") {
            ServiceStartType::Disabled
        } else {
            ServiceStartType::Manual
        };

        let (_, query_out) = Self::run_cmd("sc.exe", &["query", service_name])?;
        let status = if query_out.contains("RUNNING") {
            ServiceStatus::Running
        } else {
            ServiceStatus::Stopped
        };

        Ok(Some(ServiceState { start_type, status }))
    }

    fn set_service_state(
        &mut self,
        service_name: &str,
        state: ServiceState,
    ) -> Result<(), ShieldError> {
        let start_arg = match state.start_type {
            ServiceStartType::Automatic => "auto",
            ServiceStartType::Manual => "demand",
            ServiceStartType::Disabled => "disabled",
        };

        let (config_ok, out) =
            Self::run_cmd("sc.exe", &["config", service_name, "start=", start_arg])?;
        if !config_ok {
            return Err(ShieldError::Backend(format!(
                "Failed to configure service {service_name}: {out}"
            )));
        }

        match state.status {
            ServiceStatus::Stopped => {
                let _ = Self::run_cmd("sc.exe", &["stop", service_name]);
            }
            ServiceStatus::Running => {
                let _ = Self::run_cmd("sc.exe", &["start", service_name]);
            }
        }

        Ok(())
    }

    fn get_task_state(&self, task_path: &str) -> Result<Option<TaskState>, ShieldError> {
        let (success, output) = Self::run_cmd("schtasks.exe", &["/query", "/tn", task_path])?;
        if !success {
            return Ok(None);
        }

        let enabled = !output.contains("Disabled");
        Ok(Some(TaskState { enabled }))
    }

    fn set_task_state(&mut self, task_path: &str, state: TaskState) -> Result<(), ShieldError> {
        let arg = if state.enabled { "/enable" } else { "/disable" };
        let (success, out) = Self::run_cmd("schtasks.exe", &["/change", "/tn", task_path, arg])?;

        if !success {
            return Err(ShieldError::Backend(format!(
                "Failed to change scheduled task {task_path} to {arg}: {out}"
            )));
        }

        Ok(())
    }

    fn read_hosts_file(&self) -> Result<String, ShieldError> {
        match std::fs::read_to_string(&self.hosts_path) {
            Ok(content) => Ok(content),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
            Err(e) => Err(ShieldError::Io(format!(
                "Failed reading hosts file at {}: {e}",
                self.hosts_path.display()
            ))),
        }
    }

    fn write_hosts_file(&mut self, content: &str) -> Result<(), ShieldError> {
        std::fs::write(&self.hosts_path, content).map_err(|e| {
            ShieldError::Io(format!(
                "Failed writing hosts file at {}: {e}",
                self.hosts_path.display()
            ))
        })
    }
}
