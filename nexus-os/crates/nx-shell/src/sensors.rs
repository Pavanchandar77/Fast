//! Island sensors. Fast pollers run on the 1 Hz island tick; slow or
//! networked ones (weather, calendar, Bluetooth batteries, now playing +
//! lyrics) run on background threads and post over channels.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone, Debug, PartialEq)]
pub enum SensorEvent {
    NetRate {
        down: f64,
        up: f64,
    },
    MediaInUse {
        app: String,
        mic: bool,
        cam: bool,
    },
    MediaIdle,
    CapsLock(bool),
    Layout(String),
    DriveAdded(String),
    DriveRemoved(String),
    Screenshot(PathBuf),
    SystemVolume {
        level: f32,
        muted: bool,
    },
    Fullscreen(bool),
    WindowsBuild {
        version: String,
        previous: Option<String>,
        reboot_pending: bool,
        probes_ok: usize,
        probes_total: usize,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum SlowEvent {
    Weather {
        place: String,
        temp_c: f32,
        summary: String,
    },
    NextEvent {
        title: String,
        starts_in_min: i64,
    },
    NoEvent,
    BluetoothDevices(Vec<(String, Option<u8>)>),
    NowPlaying(Option<NowPlaying>),
    Lyrics {
        title: String,
        lines: Vec<(f32, String)>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub playing: bool,
    pub position: f32,
    pub duration: f32,
}

pub struct Sensors {
    net_prev: Option<(u64, u64, Instant)>,
    media: Option<(String, bool, bool)>,
    caps: Option<bool>,
    layout: Option<String>,
    drives: Option<BTreeSet<String>>,
    shot_seen: Option<SystemTime>,
    volume: Option<(i32, bool)>,
    fullscreen: Option<bool>,
    build_checked: Option<Instant>,
}

impl Default for Sensors {
    fn default() -> Self {
        Self::new()
    }
}

impl Sensors {
    pub fn new() -> Self {
        Self {
            net_prev: None,
            media: None,
            caps: None,
            layout: None,
            drives: None,
            shot_seen: newest_screenshot().map(|(_, t)| t),
            volume: None,
            fullscreen: None,
            build_checked: None,
        }
    }

    /// Poll fast sensors; only changes are reported (plus the net rate).
    pub fn poll(&mut self) -> Vec<SensorEvent> {
        let mut out = Vec::new();
        if let Some((rx, tx)) = platform::net_bytes() {
            let now = Instant::now();
            if let Some((prx, ptx, pt)) = self.net_prev {
                let dt = now.duration_since(pt).as_secs_f64().max(0.001);
                out.push(SensorEvent::NetRate {
                    down: rx.saturating_sub(prx) as f64 / dt,
                    up: tx.saturating_sub(ptx) as f64 / dt,
                });
            }
            self.net_prev = Some((rx, tx, now));
        }
        let media = platform::media_in_use();
        if media != self.media {
            out.push(match &media {
                Some((app, mic, cam)) => SensorEvent::MediaInUse {
                    app: app.clone(),
                    mic: *mic,
                    cam: *cam,
                },
                None => SensorEvent::MediaIdle,
            });
            self.media = media;
        }
        if let Some(c) = platform::caps_lock() {
            if self.caps.is_some_and(|p| p != c) {
                out.push(SensorEvent::CapsLock(c));
            }
            self.caps = Some(c);
        }
        if let Some(l) = platform::keyboard_layout() {
            if self.layout.as_ref().is_some_and(|p| *p != l) {
                out.push(SensorEvent::Layout(l.clone()));
            }
            self.layout = Some(l);
        }
        let drives = platform::removable_drives();
        if let Some(prev) = &self.drives {
            for d in drives.difference(prev) {
                out.push(SensorEvent::DriveAdded(d.clone()));
            }
            for d in prev.difference(&drives) {
                out.push(SensorEvent::DriveRemoved(d.clone()));
            }
        }
        self.drives = Some(drives);
        if let Some((path, t)) = newest_screenshot() {
            if self.shot_seen.is_none_or(|s| t > s) {
                if self.shot_seen.is_some() {
                    out.push(SensorEvent::Screenshot(path));
                }
                self.shot_seen = Some(t);
            }
        }
        if let Some((v, m)) = nx_assistant::exec::read_system_volume() {
            let key = ((v * 100.0).round() as i32, m);
            if self.volume.is_some_and(|p| p != key) {
                out.push(SensorEvent::SystemVolume { level: v, muted: m });
            }
            self.volume = Some(key);
        }
        if let Some(fs) = platform::fullscreen_active() {
            if self.fullscreen != Some(fs) {
                out.push(SensorEvent::Fullscreen(fs));
            }
            self.fullscreen = Some(fs);
        }
        if self
            .build_checked
            .is_none_or(|t| t.elapsed() > Duration::from_secs(300))
        {
            self.build_checked = Some(Instant::now());
            if let Some(ev) = windows_build_status() {
                out.push(ev);
            }
        }
        out
    }
}

/// Windows build + update state from nx-compat. Reports on first poll and on
/// any change since the last Nexus run (persisted in the data dir).
pub fn windows_build_status() -> Option<SensorEvent> {
    if !cfg!(windows) {
        return None;
    }
    let info = nx_compat::build::detect_build();
    let version = info.full_version_string();
    let marker = nx_assistant::audit::data_dir().join("last_windows_build.txt");
    let previous = std::fs::read_to_string(&marker)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|p| !p.is_empty() && *p != version);
    let _ = std::fs::create_dir_all(marker.parent().unwrap_or(Path::new(".")));
    let _ = std::fs::write(&marker, &version);
    let probes = nx_compat::global_probe_manager().probe_all();
    let probes_ok = probes.iter().filter(|p| p.status.is_operational()).count();
    Some(SensorEvent::WindowsBuild {
        version,
        previous,
        reboot_pending: platform::reboot_pending(),
        probes_ok,
        probes_total: probes.len(),
    })
}

fn home() -> PathBuf {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn newest_screenshot() -> Option<(PathBuf, SystemTime)> {
    let dir = home().join("Pictures").join("Screenshots");
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("png") || x.eq_ignore_ascii_case("jpg"))
        })
        .filter_map(|e| Some((e.path(), e.metadata().ok()?.modified().ok()?)))
        .max_by_key(|(_, t)| *t)
}

/// Weather (network-gated), calendar, Bluetooth batteries every ~20s.
pub fn spawn_slow(network: Arc<AtomicBool>) -> Receiver<SlowEvent> {
    let (tx, rx) = channel();
    let _ = std::thread::Builder::new()
        .name("nexus-island-slow".into())
        .spawn(move || {
            let mut last_weather: Option<Instant> = None;
            let mut last_bt: Option<Instant> = None;
            loop {
                let net = network.load(Ordering::SeqCst);
                if net && last_weather.is_none_or(|t| t.elapsed() > Duration::from_secs(1800)) {
                    let city = nx_assistant::agent::default_city();
                    if let Ok(w) = nx_assistant::web::weather(&city) {
                        nx_assistant::audit::log(
                            "network",
                            "weather_glance",
                            &city,
                            true,
                            "island",
                        );
                        last_weather = Some(Instant::now());
                        let ev = SlowEvent::Weather {
                            place: w.place.clone(),
                            temp_c: w.temp_c,
                            summary: w.summary().to_string(),
                        };
                        if tx.send(ev).is_err() {
                            return;
                        }
                    }
                }
                let ev = match next_calendar_event(&calendar_dir()) {
                    Some((title, mins)) => SlowEvent::NextEvent {
                        title,
                        starts_in_min: mins,
                    },
                    None => SlowEvent::NoEvent,
                };
                if tx.send(ev).is_err() {
                    return;
                }
                if last_bt.is_none_or(|t| t.elapsed() > Duration::from_secs(30)) {
                    last_bt = Some(Instant::now());
                    if let Some(list) = platform::bluetooth_batteries() {
                        if tx.send(SlowEvent::BluetoothDevices(list)).is_err() {
                            return;
                        }
                    }
                }
                std::thread::sleep(Duration::from_secs(20));
            }
        });
    rx
}

/// Now playing (1 Hz) + synced lyrics (LRCLIB, network-gated, once per track).
pub fn spawn_media(network: Arc<AtomicBool>) -> Receiver<SlowEvent> {
    let (tx, rx) = channel();
    let _ = std::thread::Builder::new()
        .name("nexus-island-media".into())
        .spawn(move || {
            let mut last_track = String::new();
            loop {
                let np = platform::now_playing();
                if let Some(n) = &np {
                    let key = format!("{}\u{1}{}", n.artist, n.title);
                    if key != last_track {
                        last_track = key;
                        if network.load(Ordering::SeqCst) && !n.title.is_empty() {
                            nx_assistant::audit::log("network", "lyrics", &n.title, true, "island");
                            let lines =
                                nx_assistant::web::lyrics(&n.artist, &n.title).unwrap_or_default();
                            let _ = tx.send(SlowEvent::Lyrics {
                                title: n.title.clone(),
                                lines,
                            });
                        }
                    }
                }
                if tx.send(SlowEvent::NowPlaying(np)).is_err() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(1000));
            }
        });
    rx
}

/// Global cursor direction relative to the island (-1..1), when the OS
/// exposes the pointer outside our window (Windows). Drives the eyes' gaze.
pub fn cursor_direction(window: &gpui::Window) -> Option<(f32, f32)> {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let mut p = POINT::default();
        unsafe { GetCursorPos(&mut p).ok()? };
        let b = window.bounds();
        let scale = window.scale_factor();
        let cx = (f32::from(b.origin.x) + f32::from(b.size.width) * 0.5) * scale;
        let cy = (f32::from(b.origin.y) + 26.0) * scale;
        let dx = (p.x as f32 - cx) / (600.0 * scale);
        let dy = (p.y as f32 - cy) / (400.0 * scale);
        Some((dx.clamp(-1.0, 1.0), dy.clamp(-1.0, 1.0)))
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        None
    }
}

/// Play/pause, next, previous on the current media session.
pub fn media_command(cmd: MediaCmd) -> bool {
    platform::media_command(cmd)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaCmd {
    PlayPause,
    Next,
    Previous,
}

pub fn calendar_dir() -> PathBuf {
    nx_assistant::audit::data_dir().join("calendar")
}

/// Local UTC offset in seconds.
pub fn local_offset_secs() -> i64 {
    platform::local_offset_secs()
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Parse an ICS DTSTART value to UTC epoch seconds.
pub fn parse_ics_time(v: &str, offset: i64) -> Option<i64> {
    let v = v.trim();
    let utc = v.ends_with('Z');
    let v = v.trim_end_matches('Z');
    let (date, time) = v.split_once('T').unwrap_or((v, "000000"));
    if date.len() != 8 || time.len() < 4 {
        return None;
    }
    let y: i64 = date[0..4].parse().ok()?;
    let m: i64 = date[4..6].parse().ok()?;
    let d: i64 = date[6..8].parse().ok()?;
    let hh: i64 = time[0..2].parse().ok()?;
    let mm: i64 = time[2..4].parse().ok()?;
    let ss: i64 = time.get(4..6).and_then(|s| s.parse().ok()).unwrap_or(0);
    let secs = days_from_civil(y, m, d) * 86_400 + hh * 3600 + mm * 60 + ss;
    Some(if utc { secs } else { secs - offset })
}

/// VEVENTs from ICS text: (start_utc, summary).
pub fn parse_ics(text: &str, offset: i64) -> Vec<(i64, String)> {
    let mut out = Vec::new();
    let mut start = None;
    let mut summary = String::new();
    for raw in text.lines() {
        let line = raw.trim_end_matches('\r');
        if line == "BEGIN:VEVENT" {
            start = None;
            summary.clear();
        } else if let Some(rest) = line.strip_prefix("DTSTART") {
            if let Some((_, v)) = rest.split_once(':') {
                start = parse_ics_time(v, offset);
            }
        } else if let Some(rest) = line.strip_prefix("SUMMARY") {
            if let Some((_, v)) = rest.split_once(':') {
                summary = v.to_string();
            }
        } else if line == "END:VEVENT" {
            if let Some(s) = start {
                out.push((s, summary.clone()));
            }
        }
    }
    out
}

/// Next event in the next 24h from any `*.ics` in `dir`.
pub fn next_calendar_event(dir: &Path) -> Option<(String, i64)> {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    let offset = local_offset_secs();
    let mut best: Option<(i64, String)> = None;
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        if e.path().extension().is_none_or(|x| x != "ics") {
            continue;
        }
        let text = std::fs::read_to_string(e.path()).unwrap_or_default();
        for (s, title) in parse_ics(&text, offset) {
            if s >= now - 300 && s - now <= 86_400 && best.as_ref().is_none_or(|b| s < b.0) {
                best = Some((s, title));
            }
        }
    }
    best.map(|(s, t)| (t, (s - now) / 60))
}

#[cfg(not(windows))]
mod platform {
    use super::*;

    pub fn net_bytes() -> Option<(u64, u64)> {
        let text = std::fs::read_to_string("/proc/net/dev").ok()?;
        let mut rx = 0u64;
        let mut tx = 0u64;
        for line in text.lines().skip(2) {
            let (name, rest) = line.split_once(':')?;
            if name.trim() == "lo" {
                continue;
            }
            let f: Vec<u64> = rest
                .split_whitespace()
                .filter_map(|s| s.parse().ok())
                .collect();
            if f.len() >= 9 {
                rx += f[0];
                tx += f[8];
            }
        }
        Some((rx, tx))
    }
    pub fn media_in_use() -> Option<(String, bool, bool)> {
        None
    }
    pub fn caps_lock() -> Option<bool> {
        None
    }
    pub fn keyboard_layout() -> Option<String> {
        None
    }
    pub fn fullscreen_active() -> Option<bool> {
        None
    }
    pub fn removable_drives() -> BTreeSet<String> {
        let text = std::fs::read_to_string("/proc/mounts").unwrap_or_default();
        text.lines()
            .filter_map(|l| l.split_whitespace().nth(1))
            .filter(|m| m.starts_with("/media/") || m.starts_with("/run/media/"))
            .map(|m| m.rsplit('/').next().unwrap_or(m).to_string())
            .collect()
    }
    pub fn reboot_pending() -> bool {
        false
    }
    pub fn bluetooth_batteries() -> Option<Vec<(String, Option<u8>)>> {
        None
    }
    pub fn now_playing() -> Option<NowPlaying> {
        None
    }
    pub fn media_command(_cmd: MediaCmd) -> bool {
        false
    }
    pub fn local_offset_secs() -> i64 {
        #[repr(C)]
        struct Tm {
            tm_sec: i32,
            tm_min: i32,
            tm_hour: i32,
            tm_mday: i32,
            tm_mon: i32,
            tm_year: i32,
            tm_wday: i32,
            tm_yday: i32,
            tm_isdst: i32,
            tm_gmtoff: i64,
            tm_zone: *const i8,
        }
        unsafe extern "C" {
            fn time(tloc: *mut i64) -> i64;
            fn localtime_r(timep: *const i64, result: *mut Tm) -> *mut Tm;
        }
        unsafe {
            let mut t = 0i64;
            time(&mut t);
            let mut tm = std::mem::zeroed::<Tm>();
            if localtime_r(&t, &mut tm).is_null() {
                0
            } else {
                tm.tm_gmtoff
            }
        }
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::{ERROR_SUCCESS, HWND, RECT};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER,
        HKEY_LOCAL_MACHINE, KEY_READ,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn open(root: HKEY, path: &str) -> Option<HKEY> {
        let w = wide(path);
        let mut h = HKEY::default();
        let rc = unsafe { RegOpenKeyExW(root, PCWSTR(w.as_ptr()), 0, KEY_READ, &mut h) };
        (rc == ERROR_SUCCESS).then_some(h)
    }

    fn subkeys(h: HKEY) -> Vec<String> {
        let mut out = Vec::new();
        for i in 0..512u32 {
            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            let rc = unsafe {
                RegEnumKeyExW(
                    h,
                    i,
                    PWSTR(buf.as_mut_ptr()),
                    &mut len,
                    None,
                    PWSTR::null(),
                    None,
                    None,
                )
            };
            if rc != ERROR_SUCCESS {
                break;
            }
            out.push(String::from_utf16_lossy(&buf[..len as usize]));
        }
        out
    }

    fn qword(h: HKEY, name: &str) -> Option<u64> {
        let w = wide(name);
        let mut v = 0u64;
        let mut len = 8u32;
        let rc = unsafe {
            RegQueryValueExW(
                h,
                PCWSTR(w.as_ptr()),
                None,
                None,
                Some(&mut v as *mut u64 as *mut u8),
                Some(&mut len),
            )
        };
        (rc == ERROR_SUCCESS).then_some(v)
    }

    /// Apps currently using a capability per CapabilityAccessManager.
    fn active_apps(cap: &str) -> Vec<String> {
        let base = format!(
            r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\{cap}"
        );
        let mut apps = Vec::new();
        let mut scan = |path: &str, nonpackaged: bool| {
            let Some(h) = open(HKEY_CURRENT_USER, path) else {
                return;
            };
            for k in subkeys(h) {
                if k == "NonPackaged" {
                    continue;
                }
                if let Some(sub) = open(h, &k) {
                    let start = qword(sub, "LastUsedTimeStart").unwrap_or(0);
                    let stop = qword(sub, "LastUsedTimeStop").unwrap_or(1);
                    if start > 0 && stop == 0 {
                        let name = if nonpackaged {
                            k.rsplit('#')
                                .next()
                                .unwrap_or(&k)
                                .trim_end_matches(".exe")
                                .to_string()
                        } else {
                            k.split('_')
                                .next()
                                .unwrap_or(&k)
                                .rsplit('.')
                                .next()
                                .unwrap_or(&k)
                                .to_string()
                        };
                        apps.push(name);
                    }
                    unsafe {
                        let _ = RegCloseKey(sub);
                    }
                }
            }
            unsafe {
                let _ = RegCloseKey(h);
            }
        };
        scan(&base, false);
        scan(&format!(r"{base}\NonPackaged"), true);
        apps
    }

    pub fn media_in_use() -> Option<(String, bool, bool)> {
        let mic = active_apps("microphone");
        let cam = active_apps("webcam");
        if mic.is_empty() && cam.is_empty() {
            return None;
        }
        let app = cam.first().or(mic.first()).cloned().unwrap_or_default();
        Some((pretty_app(&app), !mic.is_empty(), !cam.is_empty()))
    }

    fn pretty_app(a: &str) -> String {
        let l = a.to_ascii_lowercase();
        for (k, v) in [
            ("teams", "Teams"),
            ("zoom", "Zoom"),
            ("chrome", "Chrome"),
            ("msedge", "Edge"),
            ("firefox", "Firefox"),
            ("discord", "Discord"),
            ("slack", "Slack"),
            ("obs", "OBS"),
        ] {
            if l.contains(k) {
                return v.to_string();
            }
        }
        a.to_string()
    }

    pub fn net_bytes() -> Option<(u64, u64)> {
        use windows::Win32::NetworkManagement::IpHelper::{
            FreeMibTable, GetIfTable2, MIB_IF_TABLE2,
        };
        unsafe {
            let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
            if GetIfTable2(&mut table).is_err() || table.is_null() {
                return None;
            }
            let n = (*table).NumEntries as usize;
            let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), n);
            let mut rx = 0u64;
            let mut tx = 0u64;
            for r in rows {
                // Skip loopback (24) and interfaces that are not up (1).
                if r.Type == 24 || r.OperStatus.0 != 1 {
                    continue;
                }
                rx += r.InOctets;
                tx += r.OutOctets;
            }
            FreeMibTable(table as *const _);
            Some((rx, tx))
        }
    }

    pub fn caps_lock() -> Option<bool> {
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CAPITAL};
        Some(unsafe { GetKeyState(VK_CAPITAL.0 as i32) } & 1 == 1)
    }

    pub fn keyboard_layout() -> Option<String> {
        use windows::Win32::Globalization::LCIDToLocaleName;
        use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId,
        };
        unsafe {
            let fg: HWND = GetForegroundWindow();
            let tid = GetWindowThreadProcessId(fg, None);
            let hkl = GetKeyboardLayout(tid);
            let lang = (hkl.0 as usize & 0xFFFF) as u32;
            let mut buf = [0u16; 85];
            let n = LCIDToLocaleName(lang, Some(&mut buf), 0);
            if n <= 1 {
                return None;
            }
            Some(String::from_utf16_lossy(&buf[..(n - 1) as usize]))
        }
    }

    /// Fullscreen per the shell (D3D fullscreen / presentation / busy), or a
    /// foreign foreground window covering its whole monitor.
    pub fn fullscreen_active() -> Option<bool> {
        use windows::Win32::Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
        };
        use windows::Win32::UI::Shell::{
            SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE,
            QUNS_RUNNING_D3D_FULL_SCREEN,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            GetClassNameW, GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId,
        };
        unsafe {
            if let Ok(state) = SHQueryUserNotificationState() {
                if state == QUNS_RUNNING_D3D_FULL_SCREEN
                    || state == QUNS_BUSY
                    || state == QUNS_PRESENTATION_MODE
                {
                    return Some(true);
                }
            }
            let fg = GetForegroundWindow();
            if fg.0.is_null() {
                return Some(false);
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(fg, Some(&mut pid));
            if pid == std::process::id() {
                return Some(false);
            }
            let mut cls = [0u16; 64];
            let n = GetClassNameW(fg, &mut cls) as usize;
            let cls = String::from_utf16_lossy(&cls[..n]);
            if cls == "Progman" || cls == "WorkerW" || cls == "Shell_TrayWnd" {
                return Some(false);
            }
            let mut r = RECT::default();
            if GetWindowRect(fg, &mut r).is_err() {
                return Some(false);
            }
            let mon = MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST);
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let _ = GetMonitorInfoW(mon, &mut mi);
            let m = mi.rcMonitor;
            Some(r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom)
        }
    }

    pub fn removable_drives() -> BTreeSet<String> {
        use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
        let mut out = BTreeSet::new();
        let mask = unsafe { GetLogicalDrives() };
        for i in 0..26u32 {
            if mask & (1 << i) == 0 {
                continue;
            }
            let letter = (b'A' + i as u8) as char;
            let root = wide(&format!("{letter}:\\"));
            // 2 = DRIVE_REMOVABLE
            if unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) } == 2 {
                out.insert(format!("{letter}:"));
            }
        }
        out
    }

    pub fn reboot_pending() -> bool {
        let p =
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\RebootRequired";
        match open(HKEY_LOCAL_MACHINE, p) {
            Some(h) => {
                unsafe {
                    let _ = RegCloseKey(h);
                }
                true
            }
            None => false,
        }
    }

    /// Connected Bluetooth devices with battery % (PnP property), via PowerShell.
    pub fn bluetooth_batteries() -> Option<Vec<(String, Option<u8>)>> {
        use std::os::windows::process::CommandExt;
        let script = "Get-PnpDevice -Class Bluetooth -Status OK -ErrorAction SilentlyContinue | ForEach-Object { $b = (Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName '{104EA319-6EE2-4701-BD47-8DDBF425BBE5} 2' -ErrorAction SilentlyContinue).Data; if ($b -ne $null) { \"$($_.FriendlyName)|$b\" } }";
        let out = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .creation_flags(0x0800_0000)
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        Some(
            text.lines()
                .filter_map(|l| {
                    let (n, b) = l.trim().rsplit_once('|')?;
                    Some((n.to_string(), b.trim().parse::<u8>().ok()))
                })
                .collect(),
        )
    }

    fn session() -> Option<windows::Media::Control::GlobalSystemMediaTransportControlsSession> {
        use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager as Mgr;
        let mgr = Mgr::RequestAsync().ok()?.get().ok()?;
        mgr.GetCurrentSession().ok()
    }

    pub fn now_playing() -> Option<NowPlaying> {
        use windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus as St;
        let s = session()?;
        let props = s.TryGetMediaPropertiesAsync().ok()?.get().ok()?;
        let title = props.Title().ok()?.to_string_lossy();
        let artist = props.Artist().ok()?.to_string_lossy();
        let playing = s
            .GetPlaybackInfo()
            .ok()
            .and_then(|p| p.PlaybackStatus().ok())
            .map(|st| st == St::Playing)
            .unwrap_or(false);
        let (position, duration) = s
            .GetTimelineProperties()
            .ok()
            .map(|t| {
                let pos = t.Position().map(|d| d.Duration).unwrap_or(0) as f32 / 1e7;
                let end = t.EndTime().map(|d| d.Duration).unwrap_or(0) as f32 / 1e7;
                (pos, end)
            })
            .unwrap_or((0.0, 0.0));
        Some(NowPlaying {
            title,
            artist,
            playing,
            position,
            duration,
        })
    }

    pub fn media_command(cmd: MediaCmd) -> bool {
        let Some(s) = session() else {
            return false;
        };
        let op = match cmd {
            MediaCmd::PlayPause => s.TryTogglePlayPauseAsync(),
            MediaCmd::Next => s.TrySkipNextAsync(),
            MediaCmd::Previous => s.TrySkipPreviousAsync(),
        };
        op.ok().and_then(|o| o.get().ok()).unwrap_or(false)
    }

    pub fn local_offset_secs() -> i64 {
        use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
        let mut tz = TIME_ZONE_INFORMATION::default();
        let rc = unsafe { GetTimeZoneInformation(&mut tz) };
        let dst = if rc == 2 { tz.DaylightBias } else { 0 };
        -((tz.Bias + dst) as i64) * 60
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ics_parses_local_and_utc_times() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nDTSTART:20261121T190000Z\r\nSUMMARY:Standup\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nDTSTART;TZID=Asia/Kolkata:20261121T190000\r\nSUMMARY:Dinner\r\nEND:VEVENT\r\n";
        let ev = parse_ics(ics, 19_800);
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0].1, "Standup");
        assert_eq!(ev[0].0 - ev[1].0, 19_800);
        assert_eq!(ev[0].0, 1_795_287_600);
    }

    #[test]
    fn sensors_poll_without_panicking() {
        let mut s = Sensors::new();
        let _ = s.poll();
        let second = s.poll();
        assert!(second
            .iter()
            .all(|e| !matches!(e, SensorEvent::DriveAdded(_))));
    }
}
