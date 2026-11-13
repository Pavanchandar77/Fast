use std::time::{Duration, Instant};
use sysinfo::System;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GovernorProfile {
    EcoSilent,
    Balanced,
    MaxCompute,
}

impl GovernorProfile {
    pub fn label(&self) -> &'static str {
        match self {
            Self::EcoSilent => "ECO SILENT",
            Self::Balanced => "BALANCED",
            Self::MaxCompute => "MAX COMPUTE",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            Self::EcoSilent => Self::Balanced,
            Self::Balanced => Self::MaxCompute,
            Self::MaxCompute => Self::EcoSilent,
        }
    }
}

#[derive(Clone)]
pub struct ProcessEntry {
    pub pid: u32,
    pub name: String,
    pub memory_mb: u64,
}

pub struct SystemTelemetry {
    sys: System,
    last_refresh: Instant,
    pub cpu_usage: f32,
    pub ram_used_gb: f32,
    pub ram_total_gb: f32,
    pub top_processes: Vec<ProcessEntry>,
    pub profile: GovernorProfile,
}

impl SystemTelemetry {
    pub fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();
        let mut telemetry = Self {
            sys,
            last_refresh: Instant::now() - Duration::from_secs(2),
            cpu_usage: 0.0,
            ram_used_gb: 0.0,
            ram_total_gb: 16.0,
            top_processes: Vec::new(),
            profile: GovernorProfile::MaxCompute,
        };
        telemetry.tick();
        telemetry
    }

    pub fn tick(&mut self) {
        if self.last_refresh.elapsed() < Duration::from_millis(850) {
            return;
        }
        self.last_refresh = Instant::now();
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        self.sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

        self.cpu_usage = self.sys.global_cpu_usage();
        self.ram_used_gb = self.sys.used_memory() as f32 / 1_073_741_824.0;
        self.ram_total_gb = (self.sys.total_memory() as f32 / 1_073_741_824.0).max(1.0);

        let mut procs: Vec<ProcessEntry> = self
            .sys
            .processes()
            .values()
            .map(|p| ProcessEntry {
                pid: p.pid().as_u32(),
                name: p.name().to_string_lossy().into_owned(),
                memory_mb: p.memory() / 1_048_576,
            })
            .filter(|p| p.memory_mb > 15)
            .collect();

        procs.sort_by(|a, b| b.memory_mb.cmp(&a.memory_mb));
        procs.truncate(8);
        self.top_processes = procs;
    }
}