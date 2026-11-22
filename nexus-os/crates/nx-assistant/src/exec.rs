//! Executes validated tools against the desktop. Windows uses Win32/COM/WinRT
//! directly; the Linux GPUI preview uses xdg-open/pactl/xdotool where present.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::tools::{PowerOp, Shield, Tool, WindowOp};
use crate::AssistantError;

/// UI-visible consequences the island applies to its own state.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Volume(u8),
    Brightness(u8),
    Muted(bool),
    Wifi(bool),
    Bluetooth(bool),
    Shield(Shield, bool),
    Focus(bool),
    OpenSpotlight(String),
    Timer { seconds: u32, label: String },
    Stopwatch(bool),
    Reminder { minutes: u32, text: String },
    Screenshot(PathBuf),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub message: String,
    pub effects: Vec<Effect>,
}

impl Outcome {
    fn new(message: impl Into<String>, effects: Vec<Effect>) -> Self {
        Self {
            message: message.into(),
            effects,
        }
    }
}

/// Virtual-key codes (Win32 values; also used to build xdotool names).
pub fn key_code(name: &str) -> Option<u16> {
    let n = name.trim().to_ascii_lowercase();
    Some(match n.as_str() {
        "ctrl" | "control" => 0x11,
        "alt" => 0x12,
        "shift" => 0x10,
        "win" | "super" | "cmd" | "meta" => 0x5B,
        "enter" | "return" => 0x0D,
        "tab" => 0x09,
        "esc" | "escape" => 0x1B,
        "space" => 0x20,
        "backspace" => 0x08,
        "delete" | "del" => 0x2E,
        "up" => 0x26,
        "down" => 0x28,
        "left" => 0x25,
        "right" => 0x27,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" => 0x21,
        "pagedown" => 0x22,
        _ => {
            let b = n.as_bytes();
            if b.len() == 1 && b[0].is_ascii_alphanumeric() {
                b[0].to_ascii_uppercase() as u16
            } else {
                let f = n.strip_prefix('f')?.parse::<u16>().ok()?;
                if !(1..=12).contains(&f) {
                    return None;
                }
                0x70 + f - 1
            }
        }
    })
}

pub fn parse_chord(keys: &str) -> Option<Vec<u16>> {
    let parts: Vec<&str> = keys.split(['+', ' ']).filter(|p| !p.is_empty()).collect();
    if parts.is_empty() || parts.len() > 4 {
        return None;
    }
    parts.iter().map(|p| key_code(p)).collect()
}

fn home() -> PathBuf {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Resolve friendly folder names and `~`.
pub fn resolve_path(p: &str) -> PathBuf {
    let t = p.trim().trim_matches(['"', '\'']);
    let lower = t.to_ascii_lowercase();
    let lower = lower
        .trim_start_matches("my ")
        .trim_end_matches(" folder")
        .trim();
    let known = [
        ("downloads", "Downloads"),
        ("download", "Downloads"),
        ("documents", "Documents"),
        ("docs", "Documents"),
        ("desktop", "Desktop"),
        ("pictures", "Pictures"),
        ("photos", "Pictures"),
        ("music", "Music"),
        ("videos", "Videos"),
    ];
    if let Some((_, dir)) = known.iter().find(|(k, _)| *k == lower) {
        return home().join(dir);
    }
    if lower == "home" || lower == "~" {
        return home();
    }
    if let Some(rest) = t.strip_prefix("~/").or_else(|| t.strip_prefix("~\\")) {
        return home().join(rest);
    }
    PathBuf::from(t)
}

fn app_target(app: &str) -> String {
    let a = app.trim().to_ascii_lowercase();
    let mapped = if cfg!(windows) {
        match a.as_str() {
            "calculator" | "calc" => "calc.exe",
            "notepad" => "notepad.exe",
            "paint" => "mspaint.exe",
            "terminal" | "cmd" | "command prompt" => "wt.exe",
            "powershell" => "powershell.exe",
            "settings" => "ms-settings:",
            "edge" | "browser" => "msedge.exe",
            "chrome" => "chrome.exe",
            "firefox" => "firefox.exe",
            "explorer" | "file explorer" | "files" => "explorer.exe",
            "spotify" => "spotify:",
            "store" => "ms-windows-store:",
            "task manager" => "taskmgr.exe",
            "camera" => "microsoft.windows.camera:",
            "mail" | "outlook" => "outlook.exe",
            "word" => "winword.exe",
            "excel" => "excel.exe",
            "vscode" | "vs code" | "code" => "code",
            _ => "",
        }
    } else {
        match a.as_str() {
            "files" | "explorer" | "file explorer" => "thunar",
            "terminal" => "x-terminal-emulator",
            "browser" | "chrome" | "edge" => "xdg-open https://duckduckgo.com",
            _ => "",
        }
    };
    if mapped.is_empty() {
        app.trim().to_string()
    } else {
        mapped.to_string()
    }
}

fn spawn(cmd: &str, args: &[&str]) -> Result<(), AssistantError> {
    let mut c = Command::new(cmd);
    c.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    c.spawn()
        .map(|_| ())
        .map_err(|e| AssistantError::Action(format!("{cmd}: {e}")))
}

fn run(cmd: &str, args: &[&str]) -> Result<(), AssistantError> {
    let mut c = Command::new(cmd);
    c.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    let out = c
        .output()
        .map_err(|e| AssistantError::Action(format!("{cmd}: {e}")))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(AssistantError::Action(format!(
            "{cmd} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

fn open_path(path: &Path) -> Result<(), AssistantError> {
    let s = path.to_string_lossy().to_string();
    if cfg!(windows) {
        spawn("explorer.exe", &[&s])
    } else {
        spawn("xdg-open", &[&s])
    }
}

fn pretty(path: &Path) -> String {
    let h = home();
    match path.strip_prefix(&h) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

pub fn screenshots_dir() -> PathBuf {
    home().join("Pictures").join("Screenshots")
}

/// Execute a validated, already-confirmed tool. Network tools and `reply`
/// are handled by the agent, not here.
pub fn execute(tool: &Tool) -> Result<Outcome, AssistantError> {
    tool.validate()?;
    match tool {
        Tool::OpenApp { app } => {
            let target = app_target(app);
            platform::launch(&target)?;
            Ok(Outcome::new(format!("Opened {app}"), vec![]))
        }
        Tool::OpenFolder { path } => {
            let p = resolve_path(path);
            if !p.is_dir() {
                return Err(AssistantError::Action(format!("{} not found", pretty(&p))));
            }
            open_path(&p)?;
            Ok(Outcome::new(format!("Opened {}", pretty(&p)), vec![]))
        }
        Tool::OpenFile { path } => {
            let p = resolve_path(path);
            if !p.exists() {
                return Err(AssistantError::Action(format!("{} not found", pretty(&p))));
            }
            platform::open_file(&p)?;
            Ok(Outcome::new(format!("Opened {}", pretty(&p)), vec![]))
        }
        Tool::SetVolume { level } => {
            let applied = platform::set_volume(*level).is_ok();
            Ok(Outcome::new(
                format!("Volume {level}%{}", note(applied)),
                vec![Effect::Volume(*level), Effect::Muted(false)],
            ))
        }
        Tool::Mute { on } => {
            let applied = platform::set_mute(*on).is_ok();
            Ok(Outcome::new(
                format!("{}{}", if *on { "Muted" } else { "Unmuted" }, note(applied)),
                vec![Effect::Muted(*on)],
            ))
        }
        Tool::SetBrightness { level } => {
            let applied = platform::set_brightness(*level).is_ok();
            Ok(Outcome::new(
                format!("Brightness {level}%{}", note(applied)),
                vec![Effect::Brightness(*level)],
            ))
        }
        Tool::SetWifi { on } => {
            let applied = platform::set_radio("WiFi", *on).is_ok();
            Ok(Outcome::new(
                format!("Wi-Fi {}{}", onoff(*on), note(applied)),
                vec![Effect::Wifi(*on)],
            ))
        }
        Tool::SetBluetooth { on } => {
            let applied = platform::set_radio("Bluetooth", *on).is_ok();
            Ok(Outcome::new(
                format!("Bluetooth {}{}", onoff(*on), note(applied)),
                vec![Effect::Bluetooth(*on)],
            ))
        }
        Tool::PrivacyShield { shield, on } => {
            let applied = platform::set_shield(*shield, *on).is_ok();
            Ok(Outcome::new(
                format!(
                    "{shield:?} {}{}",
                    if *on { "blocked" } else { "allowed" },
                    note(applied)
                ),
                vec![Effect::Shield(*shield, *on)],
            ))
        }
        Tool::FocusMode { on } => Ok(Outcome::new(
            format!("Focus {}", onoff(*on)),
            vec![Effect::Focus(*on)],
        )),
        Tool::Spotlight { query } => Ok(Outcome::new(
            format!("Spotlight: {query}"),
            vec![Effect::OpenSpotlight(query.clone())],
        )),
        Tool::SetTimer { seconds, label } => Ok(Outcome::new(
            format!("Timer {}", crate::tools::fmt_secs(*seconds)),
            vec![Effect::Timer {
                seconds: *seconds,
                label: label.clone(),
            }],
        )),
        Tool::Stopwatch { on } => Ok(Outcome::new(
            format!("Stopwatch {}", if *on { "running" } else { "stopped" }),
            vec![Effect::Stopwatch(*on)],
        )),
        Tool::Remind { minutes, text } => Ok(Outcome::new(
            format!("I'll remind you in {minutes} min"),
            vec![Effect::Reminder {
                minutes: *minutes,
                text: text.clone(),
            }],
        )),
        Tool::TypeText { text } => {
            platform::type_text(text)?;
            Ok(Outcome::new(
                format!("Typed {} chars", text.chars().count()),
                vec![],
            ))
        }
        Tool::PressKeys { keys } => {
            let chord =
                parse_chord(keys).ok_or_else(|| AssistantError::BadToolCall("bad chord".into()))?;
            platform::press_keys(keys, &chord)?;
            Ok(Outcome::new(format!("Pressed {keys}"), vec![]))
        }
        Tool::Window { op } => {
            platform::window_op(*op)?;
            Ok(Outcome::new(format!("Window {op:?}"), vec![]))
        }
        Tool::Screenshot {} => {
            let dir = screenshots_dir();
            std::fs::create_dir_all(&dir).map_err(|e| AssistantError::Action(e.to_string()))?;
            let file = dir.join(format!("Nexus-{}.png", unix_secs()));
            platform::screenshot(&file)?;
            Ok(Outcome::new(
                format!("Saved {}", pretty(&file)),
                vec![Effect::Screenshot(file)],
            ))
        }
        Tool::LockScreen {} => {
            platform::lock()?;
            Ok(Outcome::new("Locked", vec![]))
        }
        Tool::DeleteFile { path } => {
            let p = resolve_path(path);
            if !p.is_file() {
                return Err(AssistantError::Action(format!("{} not found", pretty(&p))));
            }
            platform::recycle(&p)?;
            Ok(Outcome::new(
                format!("Moved {} to Recycle Bin", pretty(&p)),
                vec![],
            ))
        }
        Tool::Power { op } => {
            platform::power(*op)?;
            Ok(Outcome::new(format!("{op:?}…"), vec![]))
        }
        Tool::WebSearch { .. } | Tool::Weather { .. } | Tool::Reply { .. } => Err(
            AssistantError::Action("handled by the agent, not the executor".into()),
        ),
    }
}

fn onoff(on: bool) -> &'static str {
    if on {
        "on"
    } else {
        "off"
    }
}

/// Mark actions that only updated Nexus' own state (no OS backend present).
fn note(applied: bool) -> &'static str {
    if applied {
        ""
    } else {
        " (Nexus only)"
    }
}

pub fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(not(windows))]
mod platform {
    use super::*;

    fn which(cmd: &str) -> bool {
        std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).any(|d| d.join(cmd).is_file()))
            .unwrap_or(false)
    }

    pub fn launch(target: &str) -> Result<(), AssistantError> {
        let mut parts = target.split_whitespace();
        let cmd = parts.next().unwrap_or_default();
        let args: Vec<&str> = parts.collect();
        if !which(cmd) {
            return Err(AssistantError::Action(format!("{cmd} is not installed")));
        }
        spawn(cmd, &args)
    }
    pub fn open_file(p: &Path) -> Result<(), AssistantError> {
        spawn("xdg-open", &[&p.to_string_lossy()])
    }
    pub fn set_volume(level: u8) -> Result<(), AssistantError> {
        run(
            "pactl",
            &["set-sink-volume", "@DEFAULT_SINK@", &format!("{level}%")],
        )
    }
    pub fn set_mute(on: bool) -> Result<(), AssistantError> {
        run(
            "pactl",
            &[
                "set-sink-mute",
                "@DEFAULT_SINK@",
                if on { "1" } else { "0" },
            ],
        )
    }
    pub fn set_brightness(_level: u8) -> Result<(), AssistantError> {
        Err(AssistantError::Action("no backlight in preview".into()))
    }
    pub fn set_radio(_kind: &str, _on: bool) -> Result<(), AssistantError> {
        Err(AssistantError::Action("no radios in preview".into()))
    }
    pub fn set_shield(_s: Shield, _on: bool) -> Result<(), AssistantError> {
        Err(AssistantError::Action(
            "ConsentStore is Windows-only".into(),
        ))
    }
    pub fn type_text(text: &str) -> Result<(), AssistantError> {
        run("xdotool", &["type", "--delay", "8", "--", text])
    }
    pub fn press_keys(keys: &str, _chord: &[u16]) -> Result<(), AssistantError> {
        let k = keys
            .to_ascii_lowercase()
            .replace(' ', "+")
            .replace("win", "super");
        run("xdotool", &["key", &k])
    }
    pub fn window_op(op: WindowOp) -> Result<(), AssistantError> {
        let args: &[&str] = match op {
            WindowOp::Minimize => &["getactivewindow", "windowminimize"],
            WindowOp::Close => &["getactivewindow", "windowclose"],
            WindowOp::SnapLeft => &["getactivewindow", "windowmove", "0", "0"],
            _ => &["getactivewindow", "windowactivate"],
        };
        run("xdotool", args)
    }
    pub fn screenshot(file: &Path) -> Result<(), AssistantError> {
        run("import", &["-window", "root", &file.to_string_lossy()])
    }
    pub fn lock() -> Result<(), AssistantError> {
        run("loginctl", &["lock-session"])
    }
    pub fn recycle(p: &Path) -> Result<(), AssistantError> {
        let trash = home().join(".local/share/Trash/files");
        std::fs::create_dir_all(&trash).map_err(|e| AssistantError::Action(e.to_string()))?;
        let name = p.file_name().unwrap_or_default();
        std::fs::rename(p, trash.join(name)).map_err(|e| AssistantError::Action(e.to_string()))
    }
    pub fn power(_op: PowerOp) -> Result<(), AssistantError> {
        Err(AssistantError::Action(
            "power actions are disabled in the preview".into(),
        ))
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
        KEYEVENTF_UNICODE, VIRTUAL_KEY,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindow, GetWindowTextLengthW, GetWindowThreadProcessId,
        IsWindowVisible, PostMessageW, SetForegroundWindow, SetWindowPos, ShowWindow, GW_HWNDNEXT,
        SWP_NOZORDER, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE, WM_CLOSE,
    };

    fn ps(script: &str) -> Result<(), AssistantError> {
        run(
            "powershell.exe",
            &[
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                script,
            ],
        )
    }

    pub fn launch(target: &str) -> Result<(), AssistantError> {
        spawn("cmd.exe", &["/C", "start", "", target])
    }
    pub fn open_file(p: &Path) -> Result<(), AssistantError> {
        spawn("cmd.exe", &["/C", "start", "", &p.to_string_lossy()])
    }

    fn endpoint(
    ) -> Result<windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume, AssistantError> {
        use windows::Win32::Media::Audio::{
            eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator,
        };
        use windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
        };
        let err = |e: windows::core::Error| AssistantError::Action(e.to_string());
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let en: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(err)?;
            let dev = en.GetDefaultAudioEndpoint(eRender, eConsole).map_err(err)?;
            dev.Activate(CLSCTX_ALL, None).map_err(err)
        }
    }

    pub fn read_volume() -> Option<(f32, bool)> {
        let ep = endpoint().ok()?;
        unsafe {
            let v = ep.GetMasterVolumeLevelScalar().ok()?;
            let m = ep.GetMute().ok()?.as_bool();
            Some((v, m))
        }
    }

    pub fn set_volume(level: u8) -> Result<(), AssistantError> {
        let ep = endpoint()?;
        unsafe {
            ep.SetMasterVolumeLevelScalar(level as f32 / 100.0, std::ptr::null())
                .map_err(|e| AssistantError::Action(e.to_string()))?;
            let _ = ep.SetMute(false, std::ptr::null());
        }
        Ok(())
    }
    pub fn set_mute(on: bool) -> Result<(), AssistantError> {
        let ep = endpoint()?;
        unsafe {
            ep.SetMute(on, std::ptr::null())
                .map_err(|e| AssistantError::Action(e.to_string()))
        }
    }
    #[allow(dead_code)] // kept for the brightness intent; not wired yet
    pub fn read_brightness() -> Option<u8> {
        use windows::Win32::Devices::Display::{
            GetMonitorBrightness, GetNumberOfPhysicalMonitorsFromHMONITOR,
            GetPhysicalMonitorsFromHMONITOR, PHYSICAL_MONITOR,
        };
        use windows::Win32::Foundation::{BOOL, LPARAM, RECT};
        use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};

        unsafe extern "system" fn each(
            monitor: HMONITOR,
            _dc: HDC,
            _rect: *mut RECT,
            data: LPARAM,
        ) -> BOOL {
            let list = &mut *(data.0 as *mut Vec<HMONITOR>);
            list.push(monitor);
            BOOL(1)
        }

        let mut monitors: Vec<HMONITOR> = Vec::new();
        unsafe {
            let _ = EnumDisplayMonitors(None, None, Some(each), LPARAM(&raw mut monitors as isize));
        }
        for monitor in monitors {
            let mut count = 0u32;
            unsafe {
                if GetNumberOfPhysicalMonitorsFromHMONITOR(monitor, &raw mut count).is_err()
                    || count == 0
                {
                    continue;
                }
                let mut physical = vec![PHYSICAL_MONITOR::default(); count as usize];
                if GetPhysicalMonitorsFromHMONITOR(monitor, &mut physical).is_err() {
                    continue;
                }
                for item in &physical {
                    let mut min = 0u32;
                    let mut cur = 0u32;
                    let mut max = 0u32;
                    if GetMonitorBrightness(item.hPhysicalMonitor, &mut min, &mut cur, &mut max)
                        != 0
                        && cur <= 100
                    {
                        let _ =
                            windows::Win32::Devices::Display::DestroyPhysicalMonitors(&physical);
                        return Some(cur as u8);
                    }
                }
                let _ = windows::Win32::Devices::Display::DestroyPhysicalMonitors(&physical);
            }
        }
        read_scheme_brightness()
    }

    #[allow(dead_code)]
    fn read_scheme_brightness() -> Option<u8> {
        use std::ffi::c_void;
        use windows::core::{s, w, GUID};
        use windows::Win32::Foundation::LocalFree;
        use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

        type GetScheme = unsafe extern "system" fn(*mut c_void, *mut *mut GUID) -> u32;
        type ReadIndex = unsafe extern "system" fn(
            *mut c_void,
            *const GUID,
            *const GUID,
            *const GUID,
            *mut u32,
        ) -> u32;

        unsafe {
            let lib = LoadLibraryW(w!("powrprof.dll")).ok()?;
            let get: GetScheme =
                std::mem::transmute(GetProcAddress(lib, s!("PowerGetActiveScheme"))?);
            let read: ReadIndex =
                std::mem::transmute(GetProcAddress(lib, s!("PowerReadACValueIndex"))?);
            let mut scheme = std::ptr::null_mut();
            if get(std::ptr::null_mut(), &mut scheme) != 0 || scheme.is_null() {
                return None;
            }
            let subgroup = GUID::from_u128(0x7516_b95f_f776_4464_8c53_0616_7f40_cc99);
            let setting = GUID::from_u128(0xaded_5e82_b909_4619_9949_f5d7_1dac_0bcb);
            let mut value = 0u32;
            let status = read(
                std::ptr::null_mut(),
                scheme,
                &subgroup,
                &setting,
                &mut value,
            );
            let _ = LocalFree(windows::Win32::Foundation::HLOCAL(scheme.cast()));
            (status == 0 && value <= 100).then_some(value as u8)
        }
    }

    pub fn set_brightness(level: u8) -> Result<(), AssistantError> {
        // Never PowerShell. WMI through powershell.exe is what sat for 40
        // seconds before the panel moved.
        if set_monitor_brightness(level).is_ok()
            || set_lcd_brightness(level).is_ok()
            || set_scheme_brightness(level).is_ok()
        {
            return Ok(());
        }
        Err(AssistantError::Action(
            "this display did not accept a brightness change".into(),
        ))
    }

    /// DDC brightness on each physical monitor. This is the call the panel
    /// answers in a few milliseconds.
    fn set_monitor_brightness(level: u8) -> Result<(), AssistantError> {
        use windows::Win32::Devices::Display::{
            DestroyPhysicalMonitors, GetNumberOfPhysicalMonitorsFromHMONITOR,
            GetPhysicalMonitorsFromHMONITOR, SetMonitorBrightness, PHYSICAL_MONITOR,
        };
        use windows::Win32::Foundation::{BOOL, LPARAM, RECT};
        use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};

        unsafe extern "system" fn each(
            monitor: HMONITOR,
            _dc: HDC,
            _rect: *mut RECT,
            data: LPARAM,
        ) -> BOOL {
            let list = &mut *(data.0 as *mut Vec<HMONITOR>);
            list.push(monitor);
            BOOL(1)
        }

        let mut monitors: Vec<HMONITOR> = Vec::new();
        unsafe {
            let _ = EnumDisplayMonitors(None, None, Some(each), LPARAM(&raw mut monitors as isize));
        }
        if monitors.is_empty() {
            return Err(AssistantError::Action("no monitors".into()));
        }
        let mut applied = false;
        for monitor in monitors {
            let mut count = 0u32;
            unsafe {
                if GetNumberOfPhysicalMonitorsFromHMONITOR(monitor, &raw mut count).is_err()
                    || count == 0
                {
                    continue;
                }
                let mut physical = vec![PHYSICAL_MONITOR::default(); count as usize];
                if GetPhysicalMonitorsFromHMONITOR(monitor, &mut physical).is_err() {
                    continue;
                }
                for item in &physical {
                    if SetMonitorBrightness(item.hPhysicalMonitor, u32::from(level)) != 0 {
                        applied = true;
                    }
                }
                let _ = DestroyPhysicalMonitors(&physical);
            }
        }
        if applied {
            Ok(())
        } else {
            Err(AssistantError::Action("monitor brightness refused".into()))
        }
    }

    /// `IOCTL_VIDEO_SET_DISPLAY_BRIGHTNESS` on `\\.\LCD`. Laptop panels answer
    /// this in one call.
    fn set_lcd_brightness(level: u8) -> Result<(), AssistantError> {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::Storage::FileSystem::{
            CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        };
        use windows::Win32::System::IO::DeviceIoControl;

        #[repr(C)]
        struct DisplayBrightness {
            policy: u8,
            ac: u8,
            dc: u8,
        }
        const IOCTL_VIDEO_SET_DISPLAY_BRIGHTNESS: u32 = 0x0023_049C;
        let wide: Vec<u16> = std::ffi::OsStr::new(r"\\.\LCD")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let err = |e: windows::core::Error| AssistantError::Action(e.to_string());
        unsafe {
            let handle = CreateFileW(
                PCWSTR(wide.as_ptr()),
                0x8000_0000 | 0x4000_0000,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
            .map_err(err)?;
            let body = DisplayBrightness {
                policy: 3,
                ac: level,
                dc: level,
            };
            let mut returned = 0u32;
            let result = DeviceIoControl(
                handle,
                IOCTL_VIDEO_SET_DISPLAY_BRIGHTNESS,
                Some(std::ptr::from_ref(&body).cast()),
                std::mem::size_of::<DisplayBrightness>() as u32,
                None,
                0,
                Some(&mut returned),
                None,
            );
            let _ = CloseHandle(handle);
            result.map_err(err)?;
        }
        Ok(())
    }

    /// Current power scheme brightness. Applied immediately by Windows, and it
    /// does not start PowerShell.
    fn set_scheme_brightness(level: u8) -> Result<(), AssistantError> {
        use std::ffi::c_void;
        use windows::core::{s, w, GUID};
        use windows::Win32::Foundation::LocalFree;
        use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

        type GetScheme = unsafe extern "system" fn(*mut c_void, *mut *mut GUID) -> u32;
        type WriteIndex = unsafe extern "system" fn(
            *mut c_void,
            *const GUID,
            *const GUID,
            *const GUID,
            u32,
        ) -> u32;
        type SetScheme = unsafe extern "system" fn(*mut c_void, *const GUID) -> u32;

        unsafe {
            let lib = LoadLibraryW(w!("powrprof.dll"))
                .map_err(|e| AssistantError::Action(e.to_string()))?;
            let get: GetScheme = std::mem::transmute(
                GetProcAddress(lib, s!("PowerGetActiveScheme"))
                    .ok_or_else(|| AssistantError::Action("PowerGetActiveScheme missing".into()))?,
            );
            let write: WriteIndex = std::mem::transmute(
                GetProcAddress(lib, s!("PowerWriteACValueIndex")).ok_or_else(|| {
                    AssistantError::Action("PowerWriteACValueIndex missing".into())
                })?,
            );
            let write_dc: WriteIndex = std::mem::transmute(
                GetProcAddress(lib, s!("PowerWriteDCValueIndex")).ok_or_else(|| {
                    AssistantError::Action("PowerWriteDCValueIndex missing".into())
                })?,
            );
            let set: SetScheme = std::mem::transmute(
                GetProcAddress(lib, s!("PowerSetActiveScheme"))
                    .ok_or_else(|| AssistantError::Action("PowerSetActiveScheme missing".into()))?,
            );
            let mut scheme = std::ptr::null_mut();
            if get(std::ptr::null_mut(), &mut scheme) != 0 || scheme.is_null() {
                return Err(AssistantError::Action("no active power scheme".into()));
            }
            // VIDEO subgroup and the display brightness setting.
            let subgroup = GUID::from_u128(0x7516_b95f_f776_4464_8c53_0616_7f40_cc99);
            let setting = GUID::from_u128(0xaded_5e82_b909_4619_9949_f5d7_1dac_0bcb);
            let value = u32::from(level);
            let ac = write(std::ptr::null_mut(), scheme, &subgroup, &setting, value);
            let dc = write_dc(std::ptr::null_mut(), scheme, &subgroup, &setting, value);
            let applied = set(std::ptr::null_mut(), scheme);
            let _ = LocalFree(windows::Win32::Foundation::HLOCAL(scheme.cast()));
            if ac != 0 || dc != 0 || applied != 0 {
                return Err(AssistantError::Action(
                    "power scheme rejected brightness".into(),
                ));
            }
        }
        Ok(())
    }
    pub fn set_radio(kind: &str, on: bool) -> Result<(), AssistantError> {
        let state = if on { "On" } else { "Off" };
        let script = format!(
            r#"Add-Type -AssemblyName System.Runtime.WindowsRuntime
$m = ([System.WindowsRuntimeSystemExtensions].GetMethods() | ? {{ $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' }})[0]
Function Await($t,$r){{ $x=$m.MakeGenericMethod($r).Invoke($null,@($t)); $x.Wait(-1)|Out-Null; $x.Result }}
[Windows.Devices.Radios.Radio,Windows.System.Devices,ContentType=WindowsRuntime]|Out-Null
[Windows.Devices.Radios.RadioAccessStatus,Windows.System.Devices,ContentType=WindowsRuntime]|Out-Null
[Windows.Devices.Radios.RadioState,Windows.System.Devices,ContentType=WindowsRuntime]|Out-Null
Await ([Windows.Devices.Radios.Radio]::RequestAccessAsync()) ([Windows.Devices.Radios.RadioAccessStatus])|Out-Null
$rs = Await ([Windows.Devices.Radios.Radio]::GetRadiosAsync()) ([System.Collections.Generic.IReadOnlyList[Windows.Devices.Radios.Radio]])
$r = $rs | ? {{ $_.Kind -eq '{kind}' }} | Select-Object -First 1
if (-not $r) {{ exit 2 }}
Await ($r.SetStateAsync('{state}')) ([Windows.Devices.Radios.RadioAccessStatus])|Out-Null"#
        );
        ps(&script)
    }
    pub fn set_shield(s: Shield, on: bool) -> Result<(), AssistantError> {
        let cap = match s {
            Shield::Camera => "webcam",
            Shield::Microphone => "microphone",
            Shield::Location => "location",
        };
        // Registry writes go through the never-brick journal (Platform
        // trait), so they are recorded and can be rolled back.
        use nexus_model::platform::{Platform, RegistryValueData};
        let subkey = format!(
            r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\{cap}"
        );
        let value = RegistryValueData::Sz(if on { "Deny" } else { "Allow" }.to_string());
        os_core::platform::Win32Platform::new()
            .set_registry_value("HKCU", &subkey, "Value", value)
            .map_err(|e| AssistantError::Action(format!("privacy shield: {e}")))
    }

    /// The window the user was in before the island took focus.
    fn target_window() -> Option<HWND> {
        unsafe {
            let me = std::process::id();
            let mut h = GetForegroundWindow();
            let mut guard = 0;
            while !h.0.is_null() && guard < 512 {
                let mut pid = 0u32;
                GetWindowThreadProcessId(h, Some(&mut pid));
                if pid != me && IsWindowVisible(h).as_bool() && GetWindowTextLengthW(h) > 0 {
                    return Some(h);
                }
                h = GetWindow(h, GW_HWNDNEXT).unwrap_or_default();
                guard += 1;
            }
            None
        }
    }

    fn focus_target() -> Option<HWND> {
        let h = target_window()?;
        unsafe {
            let _ = SetForegroundWindow(h);
        }
        std::thread::sleep(std::time::Duration::from_millis(120));
        Some(h)
    }

    fn key_input(vk: u16, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: scan,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    pub fn type_text(text: &str) -> Result<(), AssistantError> {
        focus_target();
        let mut inputs = Vec::new();
        for u in text.encode_utf16() {
            inputs.push(key_input(0, u, KEYEVENTF_UNICODE));
            inputs.push(key_input(0, u, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
        }
        let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
        if sent as usize == inputs.len() {
            Ok(())
        } else {
            Err(AssistantError::Action("SendInput was blocked".into()))
        }
    }
    pub fn press_keys(_keys: &str, chord: &[u16]) -> Result<(), AssistantError> {
        focus_target();
        let mut inputs: Vec<INPUT> = chord
            .iter()
            .map(|vk| key_input(*vk, 0, KEYBD_EVENT_FLAGS(0)))
            .collect();
        inputs.extend(
            chord
                .iter()
                .rev()
                .map(|vk| key_input(*vk, 0, KEYEVENTF_KEYUP)),
        );
        unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
        Ok(())
    }
    pub fn window_op(op: WindowOp) -> Result<(), AssistantError> {
        use windows::Win32::Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
        };
        let h = target_window().ok_or_else(|| AssistantError::Action("no app window".into()))?;
        unsafe {
            match op {
                WindowOp::Minimize => {
                    let _ = ShowWindow(h, SW_MINIMIZE);
                }
                WindowOp::Maximize => {
                    let _ = ShowWindow(h, SW_MAXIMIZE);
                }
                WindowOp::Restore => {
                    let _ = ShowWindow(h, SW_RESTORE);
                }
                WindowOp::Close => {
                    // WM_CLOSE lets the app prompt to save unsaved work.
                    let _ = PostMessageW(h, WM_CLOSE, WPARAM(0), LPARAM(0));
                }
                WindowOp::SnapLeft | WindowOp::SnapRight | WindowOp::Center => {
                    let mon = MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST);
                    let mut mi = MONITORINFO {
                        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                        ..Default::default()
                    };
                    let _ = GetMonitorInfoW(mon, &mut mi);
                    let RECT {
                        left,
                        top,
                        right,
                        bottom,
                    } = mi.rcWork;
                    let (w, hgt) = (right - left, bottom - top);
                    let (x, y, cw, ch) = match op {
                        WindowOp::SnapLeft => (left, top, w / 2, hgt),
                        WindowOp::SnapRight => (left + w / 2, top, w / 2, hgt),
                        _ => (left + w / 6, top + hgt / 8, w * 2 / 3, hgt * 3 / 4),
                    };
                    let _ = ShowWindow(h, SW_RESTORE);
                    let _ = SetWindowPos(h, None, x, y, cw, ch, SWP_NOZORDER);
                }
            }
        }
        Ok(())
    }
    pub fn screenshot(file: &Path) -> Result<(), AssistantError> {
        let f = file.to_string_lossy().replace('\'', "''");
        ps(&format!(
            "Add-Type -AssemblyName System.Windows.Forms,System.Drawing; $b=[System.Windows.Forms.SystemInformation]::VirtualScreen; $bmp=New-Object System.Drawing.Bitmap $b.Width,$b.Height; $g=[System.Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($b.Left,$b.Top,0,0,$bmp.Size); $bmp.Save('{f}',[System.Drawing.Imaging.ImageFormat]::Png)"
        ))
    }
    pub fn lock() -> Result<(), AssistantError> {
        unsafe {
            windows::Win32::System::Shutdown::LockWorkStation()
                .map_err(|e| AssistantError::Action(e.to_string()))
        }
    }
    pub fn recycle(p: &Path) -> Result<(), AssistantError> {
        use windows::Win32::UI::Shell::{
            SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_SILENT, FO_DELETE,
            SHFILEOPSTRUCTW,
        };
        let mut from: Vec<u16> = p.as_os_str().to_string_lossy().encode_utf16().collect();
        from.extend([0, 0]);
        let mut op = SHFILEOPSTRUCTW {
            wFunc: FO_DELETE,
            pFrom: PCWSTR(from.as_ptr()),
            fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT).0 as u16,
            ..Default::default()
        };
        let rc = unsafe { SHFileOperationW(&mut op) };
        if rc == 0 {
            Ok(())
        } else {
            Err(AssistantError::Action(format!("recycle failed ({rc})")))
        }
    }
    pub fn power(op: PowerOp) -> Result<(), AssistantError> {
        match op {
            PowerOp::Shutdown => spawn("shutdown.exe", &["/s", "/t", "0"]),
            PowerOp::Restart => spawn("shutdown.exe", &["/r", "/t", "0"]),
            PowerOp::Sleep => spawn("rundll32.exe", &["powrprof.dll,SetSuspendState", "0,1,0"]),
        }
    }
}

/// Current system volume (0..1, muted) when the OS exposes it.
pub fn read_system_volume() -> Option<(f32, bool)> {
    #[cfg(windows)]
    {
        platform::read_volume()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn brightness_read_and_write_return_within_a_second() {
        let started = std::time::Instant::now();
        let current = platform::read_brightness();
        assert!(
            started.elapsed() < std::time::Duration::from_millis(500),
            "reading brightness took {:?}",
            started.elapsed()
        );
        let Some(level) = current else {
            panic!("this machine did not report a brightness level");
        };
        let started = std::time::Instant::now();
        let result = platform::set_brightness(level);
        assert!(
            started.elapsed() < std::time::Duration::from_millis(800),
            "setting brightness took {:?}: {result:?}",
            started.elapsed()
        );
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn known_commands_route_without_a_model() {
        let started = std::time::Instant::now();
        for _ in 0..2_000 {
            assert!(matches!(
                crate::intent::route("set brightness to 40"),
                Some(crate::tools::Tool::SetBrightness { level: 40 })
            ));
        }
        assert!(
            started.elapsed() < std::time::Duration::from_millis(200),
            "routing took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn chords_parse() {
        assert_eq!(parse_chord("ctrl+s"), Some(vec![0x11, 0x53]));
        assert_eq!(parse_chord("alt + tab"), Some(vec![0x12, 0x09]));
        assert_eq!(parse_chord("f5"), Some(vec![0x74]));
        assert_eq!(parse_chord("ctrl+banana"), None);
        assert_eq!(parse_chord(""), None);
    }

    #[test]
    fn friendly_folders_resolve_under_home() {
        let p = resolve_path("my downloads folder");
        assert!(p.ends_with("Downloads"));
        assert!(resolve_path("~/x").ends_with("x"));
    }

    #[test]
    fn network_tools_are_not_executed_here() {
        assert!(execute(&Tool::WebSearch { query: "x".into() }).is_err());
    }

    #[test]
    fn island_only_tools_emit_effects() {
        let o = execute(&Tool::SetTimer {
            seconds: 90,
            label: "tea".into(),
        })
        .unwrap();
        assert_eq!(
            o.effects,
            vec![Effect::Timer {
                seconds: 90,
                label: "tea".into()
            }]
        );
        let o = execute(&Tool::FocusMode { on: true }).unwrap();
        assert_eq!(o.effects, vec![Effect::Focus(true)]);
    }
}
