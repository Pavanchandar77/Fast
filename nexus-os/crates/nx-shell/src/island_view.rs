//! GPUI Dynamic Island: expressive eyes + on-device assistant, priority-
//! stacked live activities, now playing with synced lyrics, glances, controls,
//! a drag-and-drop shelf and a clipboard peek.
//!
//! Shapes: resting pill (or a thin slit while a fullscreen app is in front),
//! short peeks for notifications/pop-ups that retract on their own, and the
//! click-opened control center. Hover only highlights.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{
    div, img, px, size, App, ClipboardItem, Context, ExternalPaths, FocusHandle, FontWeight, Hsla,
    InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Render, Styled, Window,
};
use nx_assistant::web::SearchHit;
use nx_assistant::{AgentEvent, Assistant, Effect, EyeFrame, Eyes, Mood, Phase, Tool};
use nx_privacy_dash::clipboard::SensitivePatternDetector;
use nx_ui::gpui_theme::{mono_font_family, sf_font_family};
use nx_ui::theme::Theme;
use nx_ui::{glyph_icon, GlyphIcon};

use crate::island::{BatteryAlert, IslandModel, IslandShape, ISLAND_WIDTH};
use crate::live::{fmt_clock, fmt_rate, HudKind, LiveKind, LiveState, Spring1};
use crate::sensors::{self, MediaCmd, NowPlaying, SensorEvent, SlowEvent};

fn theme_of(cx: &App) -> Theme {
    if cx.has_global::<Theme>() {
        *cx.global::<Theme>()
    } else {
        Theme::stealth()
    }
}

/// Host hooks so the island can drive other shell surfaces.
type SpotlightFn = Arc<dyn Fn(&str, &mut App)>;

#[derive(Default)]
pub struct IslandHooks {
    open_spotlight: Option<SpotlightFn>,
}

impl gpui::Global for IslandHooks {}

impl IslandHooks {
    pub fn new(open_spotlight: impl Fn(&str, &mut App) + 'static) -> Self {
        Self {
            open_spotlight: Some(Arc::new(open_spotlight)),
        }
    }
}

const WHITE: Hsla = Hsla {
    h: 0.0,
    s: 0.0,
    l: 1.0,
    a: 1.0,
};

fn ink(a: f32) -> Hsla {
    Hsla { a, ..WHITE }
}

fn black() -> Hsla {
    Hsla::from(nx_ui::Color::BLACK)
}

/// Width of the visible island body for each shape.
fn body_width(shape: IslandShape, has_activity: bool) -> f32 {
    match shape {
        IslandShape::Slit => 200.0,
        IslandShape::Pill if has_activity => 360.0,
        IslandShape::Pill => 300.0,
        IslandShape::Peek => 460.0,
        IslandShape::Expanded => 664.0,
    }
}

fn body_height(shape: IslandShape) -> f32 {
    match shape {
        IslandShape::Slit => 6.0,
        IslandShape::Pill => 40.0,
        IslandShape::Peek => 72.0,
        IslandShape::Expanded => 40.0,
    }
}

#[derive(Clone, Debug, Default)]
struct AssistantUi {
    prompt: String,
    asked: String,
    reply: String,
    phase: Option<Phase>,
    progress: f32,
    sources: Vec<SearchHit>,
    via: Option<&'static str>,
    confirm: Option<String>,
    error: Option<String>,
    busy: bool,
}

pub struct IslandView {
    pub model: IslandModel,
    pub live: LiveState,
    t0: Instant,
    eyes: Eyes,
    focus: FocusHandle,
    width: Spring1,
    height: Spring1,
    window_h: f32,
    shape: IslandShape,
    running: bool,
    last_slow_tick: f64,
    assistant: Option<Assistant>,
    ai: AssistantUi,
    network: Arc<AtomicBool>,
    sensors: Option<std::sync::mpsc::Receiver<SensorEvent>>,
    slow_rx: Option<Receiver<SlowEvent>>,
    media_rx: Option<Receiver<SlowEvent>>,
    net_rate: (f64, f64),
    weather: Option<(String, f32, String)>,
    next_event: Option<(String, i64)>,
    now_playing: Option<NowPlaying>,
    np_seen_at: f64,
    lyrics: (String, Vec<(f32, String)>),
    bluetooth: Vec<(String, Option<u8>)>,
    media_in_use: Option<(String, bool, bool)>,
    location_in_use: Option<String>,
    clip: Vec<String>,
    clip_last: Option<String>,
    detector: Option<SensitivePatternDetector>,
    pub shelf: Vec<PathBuf>,
    vol_bounds: Rc<Cell<Option<gpui::Bounds<gpui::Pixels>>>>,
    lux_bounds: Rc<Cell<Option<gpui::Bounds<gpui::Pixels>>>>,
    dragging: Option<&'static str>,
    last_applied: u8,
    /// Simulated battery (preview/demo) that host sampling must not clobber.
    battery_override: Option<(u8, bool)>,
    /// A host brightness reading arrived (later changes show the HUD).
    host_brightness_seen: bool,
    charge_step: i64,
}

impl IslandView {
    pub fn new(model: IslandModel, cx: &mut Context<Self>) -> Self {
        let shape = model.shape(0.0);
        let w = body_width(shape, false);
        let h = body_height(shape);
        Self {
            model,
            live: LiveState::default(),
            t0: Instant::now(),
            eyes: Eyes::default(),
            focus: cx.focus_handle(),
            width: Spring1::new(w),
            height: Spring1::new(h),
            window_h: shape.window_height(),
            shape,
            running: false,
            last_slow_tick: -10.0,
            assistant: None,
            ai: AssistantUi::default(),
            network: Arc::new(AtomicBool::new(nx_assistant::agent::network_enabled())),
            sensors: None,
            slow_rx: None,
            media_rx: None,
            net_rate: (0.0, 0.0),
            weather: None,
            next_event: None,
            now_playing: None,
            np_seen_at: 0.0,
            lyrics: (String::new(), Vec::new()),
            bluetooth: Vec::new(),
            media_in_use: None,
            location_in_use: None,
            clip: Vec::new(),
            clip_last: None,
            detector: None,
            shelf: Vec::new(),
            vol_bounds: Rc::new(Cell::new(None)),
            lux_bounds: Rc::new(Cell::new(None)),
            dragging: None,
            last_applied: 255,
            battery_override: None,
            host_brightness_seen: false,
            charge_step: 0,
        }
    }

    fn now(&self) -> f64 {
        self.t0.elapsed().as_secs_f64()
    }

    pub fn shape(&self) -> IslandShape {
        self.shape
    }

    pub fn set_expanded(&mut self, expanded: bool) {
        self.model.set_expanded(expanded);
    }

    pub fn set_fullscreen(&mut self, on: bool) {
        self.model.set_fullscreen(on);
    }

    /// Show a live activity and peek the island briefly (pop-ups expand then
    /// go right back to the resting pill / slit).
    pub fn notify_live(&mut self, kind: LiveKind, ttl: Option<f64>, peek: f64) {
        let now = self.now();
        if let Some(id) = self.live.push(kind, now, ttl) {
            if peek > 0.0 {
                self.live.spotlight = Some((id, now + peek));
                self.model.peek(now, peek);
            }
        }
    }

    /// Ask the on-device assistant (used by the island prompt and hotkey).
    pub fn ask(&mut self, prompt: &str) {
        let prompt = prompt.trim().to_string();
        if prompt.is_empty() {
            return;
        }
        let a = self.assistant();
        a.ask(prompt.clone());
        self.ai = AssistantUi {
            asked: prompt,
            busy: true,
            phase: Some(Phase::Thinking),
            ..Default::default()
        };
        let t = self.now() as f32;
        self.eyes.set_mood(Mood::Thinking, t, None);
    }

    fn assistant(&mut self) -> &Assistant {
        if self.assistant.is_none() {
            // No warm-up. The worker stays idle until a question needs it,
            // and the model is released again after that answer.
            let a = Assistant::spawn(self.network.load(Ordering::SeqCst));
            self.assistant = Some(a);
        }
        self.assistant.as_ref().expect("assistant")
    }

    pub fn confirm(&mut self, yes: bool) {
        if let Some(a) = &self.assistant {
            a.confirm(yes);
        }
        self.ai.confirm = None;
        self.live
            .remove_where(|k| matches!(k, LiveKind::Confirm { .. }));
    }

    pub fn open_prompt(&mut self, window: &mut Window) {
        self.model.set_expanded(true);
        window.activate_window();
        window.focus(&self.focus);
        let t = self.now() as f32;
        self.eyes.set_mood(Mood::Listening, t, None);
    }

    fn set_network(&mut self, on: bool) {
        self.network.store(on, Ordering::SeqCst);
        nx_assistant::agent::set_network_enabled(on);
        if let Some(a) = &self.assistant {
            a.set_network_allowed(on);
        }
    }

    fn apply_effect(&mut self, e: Effect, cx: &mut Context<Self>) {
        let now = self.now();
        match e {
            Effect::Volume(v) => {
                self.model.volume = v as f32 / 100.0;
                self.hud(HudKind::Volume);
            }
            Effect::Brightness(v) => {
                self.model.brightness = v as f32 / 100.0;
                self.hud(HudKind::Brightness);
            }
            Effect::Muted(m) => self.model.muted = m,
            Effect::Wifi(on) => self.model.wifi = on,
            Effect::Bluetooth(on) => self.model.bluetooth = on,
            Effect::Shield(s, on) => self.notify_live(
                LiveKind::Toast {
                    glyph: "◈",
                    title: format!("{s:?} {}", if on { "BLOCKED" } else { "ALLOWED" })
                        .to_uppercase(),
                    body: "Privacy shield".into(),
                },
                Some(4.0),
                3.0,
            ),
            Effect::Focus(on) => {
                self.live.focus = on;
                self.notify_live(
                    LiveKind::Toast {
                        glyph: "☾",
                        title: if on { "FOCUS ON" } else { "FOCUS OFF" }.into(),
                        body: String::new(),
                    },
                    Some(3.0),
                    2.5,
                );
            }
            Effect::OpenSpotlight(q) => {
                let hook = cx
                    .try_global::<IslandHooks>()
                    .and_then(|h| h.open_spotlight.clone());
                if let Some(h) = hook {
                    h(&q, cx);
                }
            }
            Effect::Timer { seconds, label } => {
                self.live.push(
                    LiveKind::Timer {
                        ends_at: now + seconds as f64,
                        total: seconds as f64,
                        label,
                    },
                    now,
                    None,
                );
            }
            Effect::Stopwatch(on) => {
                if on {
                    self.live
                        .push(LiveKind::Stopwatch { started: now }, now, None);
                } else {
                    self.live
                        .remove_where(|k| matches!(k, LiveKind::Stopwatch { .. }));
                }
            }
            Effect::Reminder { minutes, text } => {
                self.live.add_reminder(now + minutes as f64 * 60.0, text);
            }
            Effect::Screenshot(path) => {
                self.notify_live(LiveKind::Screenshot { path }, Some(6.0), 5.0);
            }
        }
    }

    fn hud(&mut self, what: HudKind) {
        let level = match what {
            HudKind::Volume => self.model.volume,
            HudKind::Brightness => self.model.brightness,
        };
        self.notify_live(
            LiveKind::Hud {
                what,
                level,
                muted: self.model.muted && what == HudKind::Volume,
            },
            Some(1.8),
            1.6,
        );
    }

    /// Run a tool directly from an island control (off the UI thread).
    fn run_tool(&self, tool: Tool) {
        std::thread::spawn(move || {
            let res = nx_assistant::exec::execute(&tool);
            let detail = match &res {
                Ok(o) => o.message.clone(),
                Err(e) => e.to_string(),
            };
            nx_assistant::audit::log(
                if res.is_ok() { "executed" } else { "failed" },
                tool.name(),
                &detail,
                false,
                "island",
            );
        });
    }

    fn drain_assistant(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(a) = &self.assistant else {
            return false;
        };
        let events: Vec<AgentEvent> = a.events.try_iter().collect();
        if events.is_empty() {
            return false;
        }
        let t = self.now() as f32;
        for e in events {
            match e {
                AgentEvent::Phase(p, prog) => {
                    if p == Phase::Ready {
                        if !self.ai.busy {
                            self.ai.phase = None;
                        }
                    } else {
                        self.ai.phase = Some(p);
                        self.ai.progress = prog;
                    }
                }
                AgentEvent::Token(tok) => {
                    self.ai.reply.push_str(&tok);
                }
                AgentEvent::Proposed {
                    tool,
                    needs_confirm,
                    via,
                } => {
                    self.ai.via = Some(via.label());
                    if needs_confirm {
                        let action = tool.describe();
                        self.ai.confirm = Some(action.clone());
                        self.eyes.set_mood(Mood::Listening, t, None);
                        self.notify_live(LiveKind::Confirm { action }, None, 0.0);
                    }
                }
                AgentEvent::Done {
                    message,
                    effects,
                    sources,
                    via,
                    ..
                } => {
                    self.ai.busy = false;
                    self.ai.phase = None;
                    self.ai.reply = message;
                    self.ai.sources = sources;
                    self.ai.via = Some(via.label());
                    self.ai.confirm = None;
                    self.live
                        .remove_where(|k| matches!(k, LiveKind::Confirm { .. }));
                    let cancelled = self.ai.reply == "Cancelled";
                    self.eyes.set_mood(
                        if cancelled { Mood::Idle } else { Mood::Happy },
                        t,
                        Some(2.4),
                    );
                    for eff in effects {
                        self.apply_effect(eff, cx);
                    }
                }
                AgentEvent::Error(msg) => {
                    self.ai.busy = false;
                    self.ai.phase = None;
                    self.ai.error = Some(msg);
                    self.ai.confirm = None;
                    self.eyes.set_mood(Mood::Error, t, Some(2.6));
                }
            }
        }
        true
    }

    /// Apply readings from the sensor thread. Cheap; runs every frame so a
    /// hardware brightness or volume key shows within a few frames.
    fn drain_sensors(&mut self) -> bool {
        let events: Vec<SensorEvent> = self
            .sensors
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        let any = !events.is_empty();
        for ev in events {
            self.sensor_event(ev);
        }
        any
    }

    fn slow_tick(&mut self, cx: &mut Context<Self>) {
        let now = self.now();
        self.model.tick();
        if let Some((pct, charging)) = self.battery_override {
            self.model.battery_pct = Some(pct);
            self.model.charging = charging;
        }
        for alert in self
            .model
            .battery_sample(self.model.battery_pct, self.model.charging)
        {
            self.battery_alert(alert);
        }
        if self.sensors.is_none() {
            self.sensors = Some(sensors::spawn_fast());
            self.slow_rx = Some(sensors::spawn_slow(self.network.clone()));
            self.media_rx = Some(sensors::spawn_media(self.network.clone()));
            self.detector = Some(SensitivePatternDetector::new());
        }
        self.drain_sensors();
        let slow: Vec<SlowEvent> = self
            .slow_rx
            .iter()
            .chain(self.media_rx.iter())
            .flat_map(|rx| rx.try_iter().collect::<Vec<_>>())
            .collect();
        for ev in slow {
            self.slow_event(ev, now);
        }
        // Clipboard peek (kept in memory only; secrets masked).
        let copied = crate::island::system_clipboard().or_else(|| {
            cx.read_from_clipboard()
                .and_then(|c| c.text())
                .map(|t| t.to_string())
        });
        if let Some(text) = copied {
            if self.clip_last.as_deref() != Some(text.as_str()) {
                self.clip_last = Some(text.clone());
                let shown = match self.detector.as_ref().and_then(|d| d.detect(&text)) {
                    Some((kind, _)) => format!("•••• {}", kind.display_name()),
                    None => text.split_whitespace().collect::<Vec<_>>().join(" "),
                };
                self.clip.retain(|c| *c != shown);
                self.clip.insert(0, shown);
                self.clip.truncate(4);
            }
        }
        self.live.tick(now);
    }

    fn battery_alert(&mut self, alert: BatteryAlert) {
        let pct = self.model.battery_pct.unwrap_or(0);
        let (title, body) = match alert {
            BatteryAlert::Below(t) => (format!("BATTERY {pct}%"), format!("Below {t}%")),
            BatteryAlert::Critical => (
                format!("BATTERY {pct}% · CRITICAL"),
                "Plug in now".to_string(),
            ),
            BatteryAlert::Plugged => (format!("CHARGING · {pct}%"), String::new()),
            BatteryAlert::Unplugged => (format!("ON BATTERY · {pct}%"), String::new()),
        };
        // Battery notices break focus mode on purpose.
        let was = self.live.focus;
        self.live.focus = false;
        self.notify_live(
            LiveKind::Toast {
                glyph: "▮",
                title,
                body,
            },
            Some(4.5),
            4.0,
        );
        self.live.focus = was;
    }

    pub fn battery_preview(&mut self, pct: u8, charging: bool) {
        self.battery_override = Some((pct, charging));
        self.model.battery_pct = Some(pct);
        self.model.charging = charging;
        for a in self.model.battery_sample(Some(pct), charging) {
            self.battery_alert(a);
        }
    }

    fn sensor_event(&mut self, ev: SensorEvent) {
        match ev {
            SensorEvent::NetRate { down, up } => self.net_rate = (down, up),
            SensorEvent::MediaInUse { app, mic, cam } => {
                let now = self.now();
                self.media_in_use = Some((app.clone(), mic, cam));
                self.notify_live(
                    LiveKind::Call {
                        app,
                        mic,
                        cam,
                        since: now,
                    },
                    None,
                    3.0,
                );
            }
            SensorEvent::LocationInUse(app) => {
                if let Some(app) = &app {
                    self.notify_live(
                        LiveKind::Toast {
                            glyph: "➤",
                            title: "LOCATION IN USE".into(),
                            body: app.to_uppercase(),
                        },
                        Some(2.4),
                        2.0,
                    );
                }
                self.location_in_use = app;
            }
            SensorEvent::MediaIdle => {
                self.media_in_use = None;
                self.live
                    .remove_where(|k| matches!(k, LiveKind::Call { .. }));
            }
            SensorEvent::CapsLock(on) => self.notify_live(
                LiveKind::Toast {
                    glyph: "⇪",
                    title: if on { "CAPS LOCK ON" } else { "CAPS LOCK OFF" }.into(),
                    body: String::new(),
                },
                Some(1.6),
                1.4,
            ),
            SensorEvent::Layout(l) => self.notify_live(
                LiveKind::Toast {
                    glyph: "⌨",
                    title: format!("KEYBOARD · {}", l.to_uppercase()),
                    body: String::new(),
                },
                Some(1.8),
                1.6,
            ),
            SensorEvent::DriveAdded(d) => self.notify_live(
                LiveKind::Toast {
                    glyph: "⏏",
                    title: format!("USB DRIVE {d}"),
                    body: "Connected".into(),
                },
                Some(4.0),
                3.0,
            ),
            SensorEvent::DriveRemoved(d) => self.notify_live(
                LiveKind::Toast {
                    glyph: "⏏",
                    title: format!("USB DRIVE {d}"),
                    body: "Removed".into(),
                },
                Some(3.0),
                2.0,
            ),
            SensorEvent::Screenshot(path) => {
                self.notify_live(LiveKind::Screenshot { path }, Some(6.0), 5.0)
            }
            SensorEvent::SystemVolume { level, muted } => {
                self.model.volume = level;
                self.model.muted = muted;
                self.hud(HudKind::Volume);
            }
            SensorEvent::Fullscreen(on) => self.model.set_fullscreen(on),
            SensorEvent::Brightness(level) => {
                let first = self.host_brightness_seen;
                self.host_brightness_seen = true;
                self.model.brightness = level;
                if first {
                    self.hud(HudKind::Brightness);
                }
            }
            SensorEvent::Disk { used, total } => {
                if total > 0 {
                    self.model.disk = (used as f32 / total as f32).clamp(0.0, 1.0);
                }
            }
            SensorEvent::WindowsBuild {
                version,
                previous,
                reboot_pending,
                probes_ok,
                probes_total,
            } => {
                let compat = format!("Nexus compat {probes_ok}/{probes_total} OK");
                if let Some(prev) = previous {
                    self.notify_live(
                        LiveKind::Update {
                            title: "WINDOWS UPDATED".into(),
                            detail: format!("{prev} → {version} · {compat}"),
                            progress: Some(1.0),
                        },
                        Some(10.0),
                        6.0,
                    );
                } else if reboot_pending {
                    self.notify_live(
                        LiveKind::Update {
                            title: "UPDATE READY · RESTART".into(),
                            detail: format!("Nexus pre-checked: {compat}"),
                            progress: None,
                        },
                        None,
                        5.0,
                    );
                }
            }
        }
    }

    fn slow_event(&mut self, ev: SlowEvent, now: f64) {
        match ev {
            SlowEvent::Weather {
                place,
                temp_c,
                summary,
            } => self.weather = Some((place, temp_c, summary)),
            SlowEvent::NextEvent {
                title,
                starts_in_min,
            } => {
                if starts_in_min == 5 && self.next_event.as_ref().is_none_or(|e| e.1 != 5) {
                    self.notify_live(
                        LiveKind::Toast {
                            glyph: "▦",
                            title: title.to_uppercase(),
                            body: "Starts in 5 min".into(),
                        },
                        Some(6.0),
                        5.0,
                    );
                }
                self.next_event = Some((title, starts_in_min));
            }
            SlowEvent::NoEvent => self.next_event = None,
            SlowEvent::BluetoothDevices(list) => {
                for (name, batt) in &list {
                    if !self.bluetooth.iter().any(|(n, _)| n == name) {
                        self.notify_live(
                            LiveKind::Bluetooth {
                                device: name.clone(),
                                battery: *batt,
                                connected: true,
                            },
                            Some(4.5),
                            4.0,
                        );
                    }
                }
                self.bluetooth = list;
            }
            SlowEvent::NowPlaying(np) => {
                self.now_playing = np;
                self.np_seen_at = now;
            }
            SlowEvent::Lyrics { title, lines } => self.lyrics = (title, lines),
        }
    }

    /// Inject now playing (+ optional synced lyrics); used by hosts and previews.
    pub fn set_now_playing(&mut self, np: Option<NowPlaying>, lyrics: Vec<(f32, String)>) {
        let title = np.as_ref().map(|n| n.title.clone()).unwrap_or_default();
        self.now_playing = np;
        self.np_seen_at = self.now();
        self.lyrics = (title, lyrics);
    }

    fn playing_position(&self) -> f32 {
        match &self.now_playing {
            Some(n) if n.playing => n.position + (self.now() - self.np_seen_at) as f32,
            Some(n) => n.position,
            None => 0.0,
        }
    }

    fn current_lyric(&self) -> Option<String> {
        let np = self.now_playing.as_ref()?;
        if self.lyrics.0 != np.title {
            return None;
        }
        nx_assistant::web::lyric_at(&self.lyrics.1, self.playing_position()).map(str::to_string)
    }

    /// Per-frame step. Returns true while something is animating.
    fn frame(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let now = self.now();
        let mut changed = false;
        if now - self.last_slow_tick >= 1.0 {
            self.last_slow_tick = now;
            self.slow_tick(cx);
            changed = true;
        }
        changed |= self.drain_assistant(cx);
        changed |= self.drain_sensors();
        if self.model.charging && self.model.battery_pct.is_some() {
            let step = (now * 3.0) as i64;
            if step != self.charge_step {
                self.charge_step = step;
                changed = true;
            }
        }

        let cursor = if self.model.hovered {
            let m = window.mouse_position();
            let b = window.bounds();
            let cx_ = f32::from(b.size.width) * 0.5;
            Some((
                (f32::from(m.x) - cx_) / 120.0,
                (f32::from(m.y) - 26.0) / 30.0,
            ))
        } else {
            sensors::cursor_direction(window)
        };
        self.eyes.update(now as f32, cursor);

        let shape = self.model.shape(now);
        let has_activity = self.live.top().is_some() || self.now_playing.is_some();
        if shape != self.shape {
            self.shape = shape;
            changed = true;
            if shape == IslandShape::Expanded {
                window.activate_window();
                window.focus(&self.focus);
            }
        }
        self.width.target = body_width(shape, has_activity);
        self.height.target = body_height(shape);
        self.width.step(1.0 / 60.0);
        self.height.step(1.0 / 60.0);
        let springs = !(self.width.settled() && self.height.settled());

        // Grow the window first, shrink it after the spring settles.
        let want = shape.window_height();
        if want > self.window_h || (!springs && want < self.window_h) {
            self.window_h = want;
            window.resize(size(px(ISLAND_WIDTH), px(want)));
        }
        let ticking = self.live.has_timer()
            || self.ai.busy
            || self.now_playing.as_ref().is_some_and(|n| n.playing);
        springs || self.eyes.animating() || changed || ticking
    }

    fn ensure_running(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.running {
            return;
        }
        self.running = true;
        cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() && this.model.expanded && !this.ai.busy {
                this.model.dismiss();
                cx.notify();
            }
        })
        .detach();
        cx.spawn_in(window, async move |this, cx| loop {
            let mut delay = Duration::from_millis(16);
            let alive = this.update_in(cx, |this, window, cx| {
                let animating = this.frame(window, cx);
                if animating {
                    cx.notify();
                } else {
                    let t = this.now() as f32;
                    let idle = this.eyes.next_event_in(t).clamp(0.016, 0.25);
                    delay = Duration::from_secs_f32(idle);
                }
            });
            if alive.is_err() {
                break;
            }
            cx.background_executor().timer(delay).await;
        })
        .detach();
    }

    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = ev.keystroke.key.as_str();
        let mods = ev.keystroke.modifiers;
        if k == "escape" {
            if self.ai.confirm.is_some() {
                self.confirm(false);
            } else {
                self.model.dismiss();
            }
            cx.notify();
            return;
        }
        if !self.model.expanded || mods.control || mods.platform || mods.function {
            return;
        }
        match k {
            "enter" => {
                if self.ai.confirm.is_some() {
                    return;
                }
                let p = std::mem::take(&mut self.ai.prompt);
                self.ask(&p);
            }
            "backspace" => {
                self.ai.prompt.pop();
            }
            "space" => self.ai.prompt.push(' '),
            _ => {
                if let Some(ch) = ev.keystroke.key_char.as_ref() {
                    if self.ai.prompt.len() < 300 {
                        self.ai.prompt.push_str(ch);
                    }
                } else if k.chars().count() == 1 && !mods.alt {
                    let c = k.chars().next().unwrap_or(' ');
                    self.ai.prompt.push(if mods.shift {
                        c.to_ascii_uppercase()
                    } else {
                        c
                    });
                }
                let t = self.now() as f32;
                if !self.ai.busy {
                    self.eyes.set_mood(Mood::Listening, t, None);
                }
            }
        }
        let _ = window;
        cx.notify();
    }
}

/// Battery fill fraction. Charging sweeps from the current level to full in
/// four steps, then restarts.
fn battery_fill_level(pct: u8, charging: bool, step: i64) -> f32 {
    let base = f32::from(pct.min(100)) / 100.0;
    if !charging || pct >= 100 {
        return base;
    }
    let phase = step.rem_euclid(5) as f32 / 4.0;
    base + (1.0 - base) * phase
}

/// Two monochrome eyes drawn from an `EyeFrame`.
pub fn eyes_element(f: EyeFrame, size_px: f32) -> impl IntoElement {
    let eye_w = size_px * 0.62;
    let eye = move |_: usize| {
        let base = div()
            .relative()
            .w(px(size_px * 0.9))
            .h(px(size_px))
            .flex()
            .items_center()
            .justify_center();
        match f.mood {
            Mood::Happy => base.child(
                div()
                    .w(px(eye_w))
                    .h(px(size_px * 0.42))
                    .mt(px(size_px * 0.2))
                    .when(size_px < 20.0, |d| d.border_t_2().border_l_2().border_r_2())
                    .when(size_px >= 20.0, |d| {
                        d.border_t_4().border_l_4().border_r_4()
                    })
                    .border_color(WHITE)
                    .rounded_t(px(size_px)),
            ),
            Mood::Error => base.child(
                div()
                    .font_family(mono_font_family())
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(size_px * 1.05))
                    .text_color(WHITE)
                    .child("×"),
            ),
            _ => {
                let open = f.openness.clamp(0.0, 1.0);
                let pulse = if f.mood == Mood::Thinking {
                    0.85 + 0.15 * (f.phase * std::f32::consts::TAU).sin()
                } else {
                    1.0
                };
                let h = (size_px * open * pulse).max(size_px * 0.12);
                let dx = f.gaze.0 * size_px * 0.18;
                let dy = f.gaze.1 * size_px * 0.14 + (size_px - h) * 0.5 - (size_px - h) * 0.5;
                base.child(
                    div()
                        .absolute()
                        .left(px((size_px * 0.9 - eye_w) * 0.5 + dx))
                        .top(px((size_px - h) * 0.5 + dy))
                        .w(px(eye_w))
                        .h(px(h))
                        .rounded(px(eye_w * 0.5))
                        .bg(WHITE),
                )
            }
        }
    };
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(size_px * 0.18))
        .child(eye(0))
        .child(eye(1))
}

fn label(text: impl Into<gpui::SharedString>, sizep: f32, a: f32) -> gpui::Div {
    div()
        .font_family(sf_font_family())
        .font_weight(FontWeight::BOLD)
        .text_size(px(sizep))
        .text_color(ink(a))
        .child(text.into())
}

fn mono(text: impl Into<gpui::SharedString>, sizep: f32, a: f32) -> gpui::Div {
    div()
        .font_family(mono_font_family())
        .text_size(px(sizep))
        .text_color(ink(a))
        .child(text.into())
}

fn bar(ratio: f32, w: f32, h: f32) -> gpui::Div {
    div().w(px(w)).h(px(h)).rounded(px(h)).bg(ink(0.16)).child(
        div()
            .h_full()
            .w(px((w * ratio.clamp(0.0, 1.0)).max(h)))
            .rounded(px(h))
            .bg(WHITE),
    )
}

const SLIDER_HEIGHT: f32 = 44.0;

/// Fill width for a pill slider: never narrower than the pill's height so
/// the rounded cap stays round.
fn slider_fill_width(ratio: f32, width: f32, h: f32) -> f32 {
    (width * ratio.clamp(0.0, 1.0)).max(h).min(width.max(h))
}

/// Level for a pointer at `x` over a track starting at `left`.
fn slider_ratio_at(x: f32, left: f32, width: f32) -> f32 {
    if width <= 0.0 {
        return 0.0;
    }
    ((x - left) / width).clamp(0.0, 1.0)
}

/// Round monochrome icon button (Control Center style).
fn icon_toggle(id: &'static str, icon: GlyphIcon, on: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .flex_none()
        .size(px(52.0))
        .rounded_full()
        .when(on, |d| d.bg(WHITE))
        .when(!on, |d| d.bg(ink(0.14)))
        .hover(|s| s.opacity(0.85))
        .cursor_pointer()
        .child(glyph_icon(icon, if on { black() } else { ink(0.85) }, 22.0))
}

fn chip(
    id: &'static str,
    text: impl Into<gpui::SharedString>,
    on: bool,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .flex_1()
        .h(px(34.0))
        .rounded(px(12.0))
        .font_family(sf_font_family())
        .font_weight(FontWeight::BOLD)
        .text_size(px(11.0))
        .when(on, |d| d.bg(WHITE).text_color(black()))
        .when(!on, |d| {
            d.border_1().border_color(ink(0.22)).text_color(ink(0.7))
        })
        .hover(|s| s.opacity(0.85))
        .child(text.into())
}

impl IslandView {
    /// Compact content for an activity (pill / peek).
    fn activity_compact(&self, kind: &LiveKind, big: bool) -> gpui::Div {
        let now = self.now();
        let t = if big { 15.0 } else { 12.0 };
        let row = div().flex().flex_row().items_center().gap(px(10.0));
        match kind {
            LiveKind::Call {
                app,
                mic,
                cam,
                since,
            } => row
                .child(label(
                    format!(
                        "● {}{}",
                        if *cam { "CAM " } else { "" },
                        if *mic { "MIC" } else { "" }
                    ),
                    t - 2.0,
                    1.0,
                ))
                .child(label(app.to_uppercase(), t, 0.85))
                .child(mono(fmt_clock(now - since), t - 1.0, 0.6)),
            LiveKind::Hud { what, level, muted } => row
                .child(label(
                    match what {
                        HudKind::Volume if *muted => "MUTE",
                        HudKind::Volume => "VOL",
                        HudKind::Brightness => "LUX",
                    },
                    t - 2.0,
                    0.7,
                ))
                .child(bar(
                    if *muted { 0.0 } else { *level },
                    if big { 260.0 } else { 160.0 },
                    6.0,
                ))
                .child(mono(
                    format!("{:>3}", (level * 100.0).round() as u8),
                    t - 1.0,
                    1.0,
                )),
            LiveKind::Timer {
                ends_at,
                total,
                label: l,
            } => {
                let left = ends_at - now;
                row.child(label(
                    if l.is_empty() {
                        "TIMER".to_string()
                    } else {
                        l.to_uppercase()
                    },
                    t - 2.0,
                    0.7,
                ))
                .child(bar(
                    (1.0 - left / total) as f32,
                    if big { 220.0 } else { 120.0 },
                    5.0,
                ))
                .child(mono(fmt_clock(left), t, 1.0))
            }
            LiveKind::Stopwatch { started } => row
                .child(label("STOPWATCH", t - 2.0, 0.7))
                .child(mono(format!("{:.1}s", now - started), t, 1.0)),
            LiveKind::Bluetooth {
                device, battery, ..
            } => row
                .child(label("ᛒ", t + 2.0, 1.0))
                .child(label(device.to_uppercase(), t - 1.0, 0.9))
                .children(battery.map(|b| {
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .child(bar(b as f32 / 100.0, 60.0, 6.0))
                        .child(mono(format!("{b}%"), t - 1.0, 0.8))
                })),
            LiveKind::Screenshot { path } => row
                .child(
                    div()
                        .w(px(if big { 84.0 } else { 34.0 }))
                        .h(px(if big { 50.0 } else { 22.0 }))
                        .rounded(px(6.0))
                        .overflow_hidden()
                        .border_1()
                        .border_color(ink(0.3))
                        .child(img(path.clone()).size_full()),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(label("SCREENSHOT", t - 1.0, 1.0))
                        .when(big, |d| {
                            d.child(mono(
                                path.file_name()
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_default(),
                                10.0,
                                0.55,
                            ))
                        }),
                ),
            LiveKind::Update {
                title,
                detail,
                progress,
            } => row.child(label("⟳", t + 1.0, 1.0)).child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.0))
                    .child(label(title.clone(), t - 1.0, 1.0))
                    .when(big, |d| d.child(mono(detail.clone(), 10.0, 0.6)))
                    .when_some(progress.filter(|_| big), |d, p| d.child(bar(p, 240.0, 4.0))),
            ),
            LiveKind::Toast { glyph, title, body } => row.child(label(*glyph, t + 1.0, 1.0)).child(
                div()
                    .flex()
                    .flex_col()
                    .child(label(title.clone(), t - 1.0, 1.0))
                    .when(big && !body.is_empty(), |d| {
                        d.child(mono(body.clone(), 11.0, 0.6))
                    }),
            ),
            LiveKind::Confirm { action } => {
                row.child(label("CONFIRM", t - 2.0, 1.0))
                    .child(mono(action.clone(), t - 2.0, 0.75))
            }
        }
    }

    fn privacy_dots(&self) -> gpui::Div {
        let (mic, cam) = self
            .media_in_use
            .as_ref()
            .map(|(_, m, c)| (*m, *c))
            .unwrap_or((false, false));
        let location = self.location_in_use.is_some();
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(4.0))
            .when(location, |d| {
                d.child(
                    div()
                        .text_size(px(8.0))
                        .text_color(WHITE)
                        .line_height(px(8.0))
                        .child("➤"),
                )
            })
            .when(cam, |d| {
                d.child(div().size(px(7.0)).rounded_full().bg(WHITE))
            })
            .when(mic, |d| {
                d.child(
                    div()
                        .size(px(7.0))
                        .rounded_full()
                        .border_1()
                        .border_color(WHITE),
                )
            })
    }

    fn battery_text(&self) -> String {
        self.model
            .battery_pct
            .map(|p| format!("{p}%"))
            .unwrap_or_else(|| "AC".to_string())
    }

    /// Battery glyph + percentage, always visible. While charging the fill
    /// sweeps up from the current level and a bolt is shown.
    fn battery_element(&self, scale: f32) -> gpui::Div {
        let Some(pct) = self.model.battery_pct else {
            return div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(4.0))
                .child(label("⚡", 10.0 * scale, 0.65))
                .child(mono("AC", 11.0 * scale, 0.65));
        };
        let level = battery_fill_level(pct, self.model.charging, self.charge_step);
        let w = 22.0 * scale;
        let h = 11.0 * scale;
        let inner = (w - 4.0) * level;
        let low = pct <= 20 && !self.model.charging;
        let body = div()
            .w(px(w))
            .h(px(h))
            .p(px(1.5))
            .rounded(px(3.0 * scale))
            .border_1()
            .border_color(ink(if low { 1.0 } else { 0.6 }))
            .child(
                div()
                    .h_full()
                    .w(px(inner.max(1.5)))
                    .rounded(px(1.5 * scale))
                    .bg(ink(if low { 1.0 } else { 0.9 })),
            );
        let nub = div()
            .w(px(2.0 * scale))
            .h(px(4.0 * scale))
            .rounded(px(1.0))
            .bg(ink(0.6));
        let mut row = div().flex().flex_row().items_center().gap(px(5.0));
        if self.model.charging {
            row = row.child(label("⚡", 10.0 * scale, 1.0));
        }
        row.child(mono(
            self.battery_text(),
            11.0 * scale,
            if low { 1.0 } else { 0.75 },
        ))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .child(body)
                .child(nub),
        )
    }

    fn render_pill(&self, peek: bool) -> gpui::Div {
        let frame = self.eyes.frame(self.now() as f32);
        let top = self.live.shown(self.now()).map(|l| l.kind.clone());
        let center: gpui::Div = match (&top, &self.now_playing) {
            (Some(k), _) => self.activity_compact(k, peek),
            (None, Some(np)) => div()
                .flex()
                .flex_row()
                .gap(px(8.0))
                .items_center()
                .child(label(if np.playing { "♪" } else { "Ⅱ" }, 12.0, 1.0))
                .child(label(np.title.to_uppercase(), 11.0, 0.9)),
            (None, None) => mono(self.model.clock_label(), 12.0, 1.0),
        };
        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .size_full()
            .px(px(14.0))
            .child(eyes_element(frame, if peek { 20.0 } else { 14.0 }))
            .child(center)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.0))
                    .child(self.privacy_dots())
                    .child(self.battery_element(if peek { 1.15 } else { 1.0 })),
            )
    }

    fn render_expanded(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let theme = theme_of(cx);
        let now = self.now();
        let frame = self.eyes.frame(now as f32);
        let ai = self.ai.clone();
        let busy_label = ai.phase.map(|p| match p {
            Phase::Downloading => format!("DOWNLOADING MODEL {:.0}%", ai.progress * 100.0),
            Phase::Loading => "WAKING UP…".into(),
            Phase::Thinking => "THINKING…".into(),
            Phase::Searching => "SEARCHING THE WEB…".into(),
            Phase::Reading => "READING SOURCES…".into(),
            Phase::Answering => "ANSWERING…".into(),
            Phase::Acting => "ON IT…".into(),
            Phase::Ready => String::new(),
        });
        let caret = if (now * 2.0).fract() < 0.5 {
            "▍"
        } else {
            " "
        };
        let prompt_line = if ai.prompt.is_empty() {
            if ai.busy {
                ai.asked.clone()
            } else {
                format!("Ask Nexus…{caret}")
            }
        } else {
            format!("{}{caret}", ai.prompt)
        };

        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(14.0))
            .h(px(52.0))
            .child(eyes_element(frame, 26.0))
            .child(
                div()
                    .flex_1()
                    .h(px(38.0))
                    .px(px(14.0))
                    .flex()
                    .items_center()
                    .rounded(px(14.0))
                    .bg(ink(0.08))
                    .border_1()
                    .border_color(ink(0.25))
                    .child(
                        div()
                            .font_family(sf_font_family())
                            .text_size(px(14.0))
                            .text_color(ink(if ai.prompt.is_empty() && !ai.busy {
                                0.45
                            } else {
                                1.0
                            }))
                            .child(prompt_line),
                    ),
            )
            .child(mono(self.model.clock_label(), 13.0, 0.8))
            .child(self.battery_element(1.1));

        let mut answer = div().flex().flex_col().gap(px(6.0));
        let show_answer =
            ai.busy || !ai.reply.is_empty() || ai.error.is_some() || ai.confirm.is_some();
        if show_answer {
            if let Some(b) = busy_label.filter(|_| ai.busy && ai.confirm.is_none()) {
                answer = answer.child(label(b, 14.0, 0.55));
            }
            if let Some(err) = &ai.error {
                answer = answer.child(label(format!("× {err}"), 13.0, 1.0));
            }
            if !ai.reply.is_empty() {
                answer = answer.child(
                    div()
                        .font_family(sf_font_family())
                        .text_size(px(14.0))
                        .text_color(WHITE)
                        .line_height(px(19.0))
                        .child(ai.reply.clone()),
                );
            }
            if let Some(action) = &ai.confirm {
                answer = answer.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(10.0))
                        .child(label(format!("CONFIRM · {action}"), 12.0, 1.0))
                        .child(
                            chip("ai-yes", "CONFIRM", true)
                                .w(px(110.0))
                                .flex_none()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.confirm(true);
                                    cx.notify();
                                })),
                        )
                        .child(
                            chip("ai-no", "CANCEL", false)
                                .w(px(100.0))
                                .flex_none()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.confirm(false);
                                    cx.notify();
                                })),
                        ),
                );
            }
            if !ai.sources.is_empty() {
                answer = answer.child(div().flex().flex_row().flex_wrap().gap(px(6.0)).children(
                    ai.sources.iter().enumerate().map(|(i, s)| {
                        let host = s
                            .url
                            .split('/')
                            .nth(2)
                            .unwrap_or(&s.url)
                            .trim_start_matches("www.")
                            .to_string();
                        div()
                            .px(px(8.0))
                            .py(px(3.0))
                            .rounded(px(8.0))
                            .border_1()
                            .border_color(ink(0.2))
                            .child(mono(format!("[{}] {host}", i + 1), 10.0, 0.75))
                    }),
                ));
            }
        }

        // Live activity stack (excluding confirm, shown above).
        let stack: Vec<(u64, LiveKind)> = self
            .live
            .stack()
            .into_iter()
            .filter(|l| !matches!(l.kind, LiveKind::Confirm { .. }))
            .map(|l| (l.id, l.kind.clone()))
            .collect();
        let activities = div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .children(stack.iter().map(|(id, k)| {
                let id = *id;
                div()
                    .px(px(12.0))
                    .py(px(8.0))
                    .rounded(px(14.0))
                    .bg(ink(0.06))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(self.activity_compact(k, true))
                    .child(
                        div()
                            .id(("forget", id as usize))
                            .flex()
                            .items_center()
                            .justify_center()
                            .w(px(28.0))
                            .h(px(28.0))
                            .rounded(px(8.0))
                            .text_size(px(14.0))
                            .text_color(ink(0.7))
                            .hover(|s| s.bg(ink(0.16)).text_color(WHITE))
                            .child("×")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.live.remove(id);
                                cx.notify();
                            })),
                    )
            }));

        let media =
            self.now_playing.clone().map(|np| {
                let pos = self.playing_position();
                let lyric = self.current_lyric();
                let frac = if np.duration > 0.0 {
                    (pos / np.duration).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let play_label = if np.playing { "Pause" } else { "Play" };
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .px(px(14.0))
                    .py(px(12.0))
                    .rounded(px(18.0))
                    .bg(Hsla {
                        h: 0.0,
                        s: 0.0,
                        l: 0.12,
                        a: 1.0,
                    })
                    .border_1()
                    .border_color(ink(0.16))
                    .child(mono("NOW PLAYING", 10.0, 0.45))
                    .child(label(np.title.clone(), 18.0, 1.0))
                    .child(mono(
                        if np.artist.is_empty() {
                            "Unknown artist".into()
                        } else {
                            np.artist.clone()
                        },
                        13.0,
                        0.7,
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.0))
                            .child(mono(fmt_clock(pos as f64), 11.0, 0.55))
                            .child(bar(frac, 420.0, 6.0))
                            .child(mono(fmt_clock(np.duration as f64), 11.0, 0.55)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_center()
                            .gap(px(8.0))
                            .child(chip("np-prev", "Previous", false).w(px(110.0)).on_click(
                                cx.listener(|_, _, _, _| {
                                    sensors::media_command(MediaCmd::Previous);
                                }),
                            ))
                            .child(chip("np-play", play_label, true).w(px(110.0)).on_click(
                                cx.listener(|this, _, _, cx| {
                                    if !sensors::media_command(MediaCmd::PlayPause) {
                                        let pos = this.lyrics_pos_snapshot();
                                        if let Some(n) = this.now_playing.as_mut() {
                                            n.position = pos;
                                            n.playing = !n.playing;
                                        }
                                        this.np_seen_at = this.now();
                                    }
                                    cx.notify();
                                }),
                            ))
                            .child(chip("np-next", "Next", false).w(px(110.0)).on_click(
                                cx.listener(|_, _, _, _| {
                                    sensors::media_command(MediaCmd::Next);
                                }),
                            )),
                    )
                    .when_some(lyric, |d, l| {
                        d.child(
                            div()
                                .font_family(sf_font_family())
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_size(px(15.0))
                                .text_color(WHITE)
                                .child(l),
                        )
                    })
            });

        let mut glance = div().flex().flex_row().gap(px(18.0)).items_center();
        if let Some((place, t, sum)) = &self.weather {
            glance = glance.child(label(
                format!("{} {:.0}°C · {sum}", place.to_uppercase(), t),
                11.0,
                0.8,
            ));
        }
        if let Some((title, mins)) = &self.next_event {
            glance = glance.child(label(
                format!("▦ {} · IN {}", title.to_uppercase(), fmt_minutes(*mins)),
                11.0,
                0.8,
            ));
        }
        glance = glance.child(mono(
            format!(
                "↓ {}  ↑ {}",
                fmt_rate(self.net_rate.0),
                fmt_rate(self.net_rate.1)
            ),
            11.0,
            0.7,
        ));

        let vol = if self.model.muted {
            0.0
        } else {
            self.model.volume
        };
        let lux = self.model.brightness;
        let meters = div()
            .flex()
            .flex_row()
            .gap(px(12.0))
            .child(self.slider(
                "vol",
                vol,
                if self.model.muted || vol <= 0.0 {
                    GlyphIcon::SpeakerMuted
                } else {
                    GlyphIcon::Speaker
                },
                self.vol_bounds.clone(),
                cx,
            ))
            .child(self.slider("lux", lux, GlyphIcon::Sun, self.lux_bounds.clone(), cx));

        let net_on = self.network.load(Ordering::SeqCst);
        let toggles = div()
            .flex()
            .flex_row()
            .justify_between()
            .child(
                icon_toggle("t-wifi", GlyphIcon::Wifi, self.model.wifi).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.model.wifi = !this.model.wifi;
                        this.run_tool(Tool::SetWifi {
                            on: this.model.wifi,
                        });
                        cx.notify();
                    },
                )),
            )
            .child(
                icon_toggle("t-bt", GlyphIcon::Bluetooth, self.model.bluetooth).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.model.bluetooth = !this.model.bluetooth;
                        this.run_tool(Tool::SetBluetooth {
                            on: this.model.bluetooth,
                        });
                        cx.notify();
                    }),
                ),
            )
            .child(
                icon_toggle(
                    "t-mute",
                    if self.model.muted {
                        GlyphIcon::SpeakerMuted
                    } else {
                        GlyphIcon::Speaker
                    },
                    !self.model.muted,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.model.muted = !this.model.muted;
                    this.run_tool(Tool::Mute {
                        on: this.model.muted,
                    });
                    this.hud(HudKind::Volume);
                    cx.notify();
                })),
            )
            .child(
                icon_toggle("t-focus", GlyphIcon::Moon, self.live.focus).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.live.focus = !this.live.focus;
                        cx.notify();
                    },
                )),
            )
            .child(
                icon_toggle("t-web", GlyphIcon::Globe, net_on).on_click(cx.listener(
                    |this, _, _, cx| {
                        let on = !this.network.load(Ordering::SeqCst);
                        this.set_network(on);
                        cx.notify();
                    },
                )),
            );

        let cpu = self.model.cpu;
        let ram = self.model.ram_ratio();
        let disk = self.model.disk.clamp(0.0, 1.0);
        let meter = |glyph: GlyphIcon, ratio: f32| {
            div()
                .flex()
                .flex_row()
                .gap(px(6.0))
                .items_center()
                .child(glyph_icon(glyph, ink(0.6), 16.0))
                .child(bar(ratio, 96.0, 6.0))
                .child(mono(
                    format!("{:>3}%", (ratio * 100.0).round() as u8),
                    11.0,
                    0.7,
                ))
        };
        let sys = div()
            .flex()
            .flex_row()
            .gap(px(12.0))
            .items_center()
            .child(meter(GlyphIcon::Cpu, cpu))
            .child(meter(GlyphIcon::Memory, ram))
            .child(meter(GlyphIcon::Folder, disk));

        let shelf_items: Vec<(usize, PathBuf)> = self.shelf.iter().cloned().enumerate().collect();
        let shelf = div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .flex_1()
            .child(glyph_icon(GlyphIcon::Tray, ink(0.5), 16.0))
            .child(div().flex().flex_row().flex_wrap().gap(px(6.0)).children(
                shelf_items.into_iter().map(|(i, p)| {
                    let name = p
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let path = p.clone();
                    div()
                        .id(("shelf", i))
                        .px(px(8.0))
                        .py(px(4.0))
                        .rounded(px(8.0))
                        .bg(ink(0.1))
                        .hover(|s| s.bg(ink(0.2)))
                        .child(mono(nx_assistant::tools::truncate(&name, 22), 10.5, 0.9))
                        .on_click(cx.listener(move |this, _, _, _| {
                            this.run_tool(Tool::OpenFile {
                                path: path.to_string_lossy().to_string(),
                            });
                        }))
                }),
            ));
        let clip = div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .flex_1()
            .child(glyph_icon(GlyphIcon::Clipboard, ink(0.5), 16.0))
            .when(self.clip.is_empty(), |d| {
                d.child(mono("Nothing copied", 10.5, 0.45))
            })
            .children(self.clip.iter().take(3).enumerate().map(|(i, c)| {
                let text = c.clone();
                div()
                    .id(("clip", i))
                    .px(px(8.0))
                    .py(px(3.0))
                    .rounded(px(8.0))
                    .hover(|s| s.bg(ink(0.12)))
                    .child(mono(nx_assistant::tools::truncate(c, 40), 10.5, 0.8))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        if !text.starts_with("••••") {
                            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                        }
                    }))
            }));

        let sep = || div().h(px(1.0)).w_full().bg(ink(0.1));
        let mut body = div().flex().flex_col().gap(px(12.0)).w_full().child(header);
        if show_answer {
            body = body.child(answer);
        }
        if !stack.is_empty() {
            body = body.child(activities);
        }
        if let Some(m) = media {
            body = body.child(m);
        }
        body = body
            .child(glance)
            .child(sep())
            .child(toggles)
            .child(meters)
            .child(sys)
            .child(sep())
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(16.0))
                    .child(shelf)
                    .child(clip),
            );
        let _ = theme;
        body
    }

    fn lyrics_pos_snapshot(&self) -> f32 {
        self.playing_position()
    }

    /// Control Center style slider: thick pill track, white fill, icon
    /// inside on the left. Click or drag anywhere on the track.
    fn slider(
        &self,
        id: &'static str,
        ratio: f32,
        icon: GlyphIcon,
        bounds: Rc<Cell<Option<gpui::Bounds<gpui::Pixels>>>>,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let h = SLIDER_HEIGHT;
        let width = bounds
            .get()
            .map(|b| f32::from(b.size.width))
            .unwrap_or(300.0);
        let fill_w = slider_fill_width(ratio, width, h);
        let icon_on_fill = fill_w >= 40.0;
        let track_bounds = bounds.clone();
        div()
            .id(id)
            .relative()
            .flex_1()
            .h(px(h))
            .rounded(px(h / 2.0))
            .bg(ink(0.16))
            .cursor_pointer()
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .h_full()
                    .w(px(fill_w))
                    .rounded(px(h / 2.0))
                    .bg(WHITE),
            )
            .child(
                div()
                    .absolute()
                    .left(px(13.0))
                    .top(px((h - 20.0) / 2.0))
                    .child(glyph_icon(
                        icon,
                        if icon_on_fill { black() } else { WHITE },
                        20.0,
                    )),
            )
            .child(
                gpui::canvas(move |b, _, _| track_bounds.set(Some(b)), |_, _, _, _| {})
                    .absolute()
                    .left_0()
                    .top_0()
                    .size_full(),
            )
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |this, ev: &gpui::MouseDownEvent, _, cx| {
                    this.dragging = Some(id);
                    this.slide_to(id, f32::from(ev.position.x), true);
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(move |this, ev: &gpui::MouseMoveEvent, _, cx| {
                if this.dragging == Some(id) && ev.pressed_button == Some(gpui::MouseButton::Left) {
                    this.slide_to(id, f32::from(ev.position.x), false);
                    cx.notify();
                }
            }))
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(move |this, ev: &gpui::MouseUpEvent, _, cx| {
                    if this.dragging == Some(id) {
                        this.slide_to(id, f32::from(ev.position.x), true);
                        this.dragging = None;
                        cx.notify();
                    }
                }),
            )
    }

    fn slide_to(&mut self, id: &str, x: f32, commit: bool) {
        let bounds = if id == "vol" {
            self.vol_bounds.get()
        } else {
            self.lux_bounds.get()
        };
        let Some(b) = bounds else {
            return;
        };
        let ratio = slider_ratio_at(x, f32::from(b.origin.x), f32::from(b.size.width));
        self.set_level(id, ratio, commit);
    }

    /// Apply a level. While dragging, the system is only touched every few
    /// percent; `commit` always applies.
    fn set_level(&mut self, id: &str, ratio: f32, commit: bool) {
        let pct = (ratio * 100.0).round() as u8;
        let apply = self.last_applied != pct && (commit || self.last_applied.abs_diff(pct) >= 4);
        if id == "vol" {
            self.model.volume = ratio;
            self.model.muted = false;
            if apply {
                self.last_applied = pct;
                self.run_tool(Tool::SetVolume { level: pct });
            }
        } else {
            self.model.brightness = ratio;
            if apply {
                self.last_applied = pct;
                self.run_tool(Tool::SetBrightness { level: pct });
            }
        }
    }
}

fn fmt_minutes(m: i64) -> String {
    if m <= 0 {
        "NOW".into()
    } else if m < 60 {
        format!("{m} MIN")
    } else {
        format!("{}H {:02}M", m / 60, m % 60)
    }
}

impl Render for IslandView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_running(window, cx);
        let shape = self.shape;
        let w = self.width.pos;
        let h = self.height.pos;
        let hovered = self.model.hovered;

        let inner: gpui::Div = match shape {
            IslandShape::Slit => div(),
            IslandShape::Pill => self.render_pill(false),
            IslandShape::Peek => self.render_pill(true),
            IslandShape::Expanded => self.render_expanded(cx),
        };
        let expanded = shape == IslandShape::Expanded;
        let radius = match shape {
            IslandShape::Slit => 3.0,
            IslandShape::Expanded => 30.0,
            _ => (h * 0.5).max(10.0),
        };

        let body = div()
            .id("island-body")
            .flex()
            .flex_col()
            .flex_none()
            .w(px(w))
            .when(!expanded, |d| d.h(px(h.max(4.0))))
            .rounded(px(radius))
            .when(shape == IslandShape::Slit, |d| d.rounded_t(px(0.0)))
            .bg(Hsla {
                h: 0.0,
                s: 0.0,
                l: 0.16,
                a: 1.0,
            })
            .border_1()
            .border_color(ink(if hovered { 0.85 } else { 0.55 }))
            .when(hovered && !expanded, |d| d.shadow_md())
            .when(expanded, |d| d.shadow_lg().px(px(18.0)).py(px(14.0)))
            .overflow_hidden()
            .on_click(cx.listener(|this, _ev, window, cx| {
                if !this.model.expanded {
                    this.model.click();
                    window.activate_window();
                    window.focus(&this.focus);
                    let t = this.now() as f32;
                    this.eyes.set_mood(Mood::Listening, t, Some(1.2));
                    cx.notify();
                }
            }))
            .drag_over::<ExternalPaths>(|s, _, _, _| s.border_color(WHITE))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                for p in paths.paths() {
                    if !this.shelf.contains(p) {
                        this.shelf.push(p.clone());
                    }
                }
                let n = this.shelf.len();
                this.notify_live(
                    LiveKind::Toast {
                        glyph: "⬚",
                        title: format!("SHELF · {n} FILE{}", if n == 1 { "" } else { "S" }),
                        body: "Held in the island".into(),
                    },
                    Some(3.0),
                    2.5,
                );
                cx.notify();
            }))
            .child(inner);

        let header_close = expanded;
        div()
            .id("island-root")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .flex()
            .flex_col()
            .items_center()
            .size_full()
            .when(shape != IslandShape::Slit, |d| d.pt(px(8.0)))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.model.hover(*hovered);
                cx.notify();
            }))
            .child(body.on_mouse_down_out(cx.listener(move |this, _, _, cx| {
                if header_close && !this.ai.busy {
                    this.model.dismiss();
                    cx.notify();
                }
            })))
            .when(expanded, |d| {
                d.child(
                    div()
                        .id("island-collapse")
                        .mt(px(6.0))
                        .w(px(46.0))
                        .h(px(5.0))
                        .rounded(px(3.0))
                        .bg(ink(0.35))
                        .hover(|s| s.bg(WHITE))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.model.dismiss();
                            cx.notify();
                        })),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    #[gpui::test]
    fn island_view_starts_as_resting_pill(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let entity = cx.new(|cx| IslandView::new(IslandModel::new(), cx));
            let view = entity.read(cx);
            assert!(!view.model.expanded);
            assert_eq!(view.shape(), IslandShape::Pill);
        });
    }

    #[gpui::test]
    fn notifications_peek_and_battery_preview_alerts(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let entity = cx.new(|cx| IslandView::new(IslandModel::new(), cx));
            entity.update(cx, |v, _| {
                v.set_fullscreen(true);
                v.notify_live(
                    LiveKind::Call {
                        app: "Zoom".into(),
                        mic: true,
                        cam: true,
                        since: 0.0,
                    },
                    None,
                    0.0,
                );
                v.battery_preview(55, false);
                v.battery_preview(49, false);
                assert_eq!(v.battery_text(), "49%");
                assert!(matches!(
                    v.live.shown(v.now()).map(|l| &l.kind),
                    Some(LiveKind::Toast { title, .. }) if title == "BATTERY 49%"
                ));
                let now = v.now();
                assert_eq!(v.model.shape(now), IslandShape::Peek);
                assert_eq!(v.model.shape(now + 10.0), IslandShape::Slit);
            });
        });
    }

    #[test]
    fn battery_fill_sweeps_only_while_charging() {
        assert_eq!(super::battery_fill_level(40, false, 3), 0.4);
        assert_eq!(super::battery_fill_level(40, true, 0), 0.4);
        assert_eq!(super::battery_fill_level(40, true, 4), 1.0);
        assert!(super::battery_fill_level(40, true, 2) > 0.4);
        assert_eq!(super::battery_fill_level(100, true, 2), 1.0);
    }

    #[test]
    fn slider_maps_pointer_and_keeps_round_cap() {
        assert_eq!(super::slider_ratio_at(150.0, 100.0, 200.0), 0.25);
        assert_eq!(super::slider_ratio_at(50.0, 100.0, 200.0), 0.0);
        assert_eq!(super::slider_ratio_at(400.0, 100.0, 200.0), 1.0);
        assert_eq!(super::slider_fill_width(0.0, 300.0, 44.0), 44.0);
        assert_eq!(super::slider_fill_width(0.5, 300.0, 44.0), 150.0);
        assert_eq!(super::slider_fill_width(1.0, 300.0, 44.0), 300.0);
    }
}
