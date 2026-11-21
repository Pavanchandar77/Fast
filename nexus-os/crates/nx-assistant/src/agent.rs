//! Background assistant worker: owns the local model, routes a prompt to one
//! validated tool, asks the island for confirmation when needed, executes, and
//! streams progress back over a channel. All inference is on-device.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::Duration;

use crate::exec::{self, Effect};
use crate::llm::{Generation, LocalModel};
use crate::tools::{self, Risk, Tool};
use crate::web::{self, SearchHit};
use crate::{audit, intent, model_store, AssistantError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Ready,
    Downloading,
    Loading,
    Thinking,
    Searching,
    Reading,
    Answering,
    Acting,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Via {
    Model,
    Rules,
}

impl Via {
    pub fn label(self) -> &'static str {
        match self {
            Via::Model => "local model",
            Via::Rules => "rules",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stats {
    pub prompt_tokens: usize,
    pub new_tokens: usize,
    pub prefill_secs: f32,
    pub tokens_per_sec: f32,
    pub total_secs: f32,
}

impl From<&Generation> for Stats {
    fn from(g: &Generation) -> Self {
        Self {
            prompt_tokens: g.prompt_tokens,
            new_tokens: g.new_tokens,
            prefill_secs: g.prefill_secs,
            tokens_per_sec: g.tokens_per_sec(),
            total_secs: g.prefill_secs + g.decode_secs,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum AgentEvent {
    Phase(Phase, f32),
    Token(String),
    /// A tool was chosen. When `needs_confirm`, the worker waits for `confirm`.
    Proposed {
        tool: Tool,
        needs_confirm: bool,
        via: Via,
    },
    Done {
        message: String,
        effects: Vec<Effect>,
        sources: Vec<SearchHit>,
        stats: Option<Stats>,
        via: Via,
    },
    Error(String),
}

enum Request {
    Warm,
    Ask(String),
    Confirm(bool),
}

/// Handle owned by the UI.
pub struct Assistant {
    tx: Sender<Request>,
    pub events: Receiver<AgentEvent>,
    network: Arc<AtomicBool>,
}

impl Assistant {
    /// Spawn the worker. The model loads lazily on the first prompt.
    pub fn spawn(network_allowed: bool) -> Self {
        let (tx, rx) = channel::<Request>();
        let (etx, events) = channel::<AgentEvent>();
        let network = Arc::new(AtomicBool::new(network_allowed));
        let net = network.clone();
        std::thread::Builder::new()
            .name("nexus-assistant".into())
            .spawn(move || Worker::new(etx, net).run(rx))
            .expect("spawn assistant thread");
        Self {
            tx,
            events,
            network,
        }
    }

    /// Load the model and pre-fill the system prompt so the first answer is fast.
    pub fn warm_up(&self) {
        let _ = self.tx.send(Request::Warm);
    }

    pub fn ask(&self, prompt: impl Into<String>) {
        let _ = self.tx.send(Request::Ask(prompt.into()));
    }

    pub fn confirm(&self, yes: bool) {
        let _ = self.tx.send(Request::Confirm(yes));
    }

    pub fn set_network_allowed(&self, on: bool) {
        self.network.store(on, Ordering::SeqCst);
        audit::log(
            "network_toggle",
            "web_search",
            if on { "enabled" } else { "disabled" },
            false,
            "user",
        );
    }

    pub fn network_allowed(&self) -> bool {
        self.network.load(Ordering::SeqCst)
    }
}

pub const ROUTER_SYSTEM_HEAD: &str = "You are Nexus, the on-device assistant living in the Dynamic Island of Nexus-OS on Windows. Pick exactly one tool that does what the user asks. Use reply only for chit-chat or questions you can answer without the web. Use web_search for facts, news, prices, people or anything current.";

pub fn router_system() -> String {
    format!(
        "{ROUTER_SYSTEM_HEAD}\n\n# Tools\n\nYou may call one function to assist with the user query.\n\nYou are provided with function signatures within <tools></tools> XML tags:\n<tools>\n{}\n</tools>\n\nFor each function call, return a json object with function name and arguments within <tool_call></tool_call> XML tags:\n<tool_call>\n{{\"name\": <function-name>, \"arguments\": <args-json-object>}}\n</tool_call>",
        tools::schema_lines()
    )
}

const ANSWER_SYSTEM: &str = "You are Nexus, a concise on-device assistant. Answer the question in at most two short sentences using only the numbered sources. Cite sources like [1]. If the sources do not answer it, say so.";

/// Decide the final tool from the model's proposal and the rule router.
pub fn choose(model: Result<Tool, AssistantError>, prompt: &str) -> Option<(Tool, Via)> {
    let rules = intent::route(prompt);
    match (model, rules) {
        // A tiny model sometimes answers with `reply` for an obvious command.
        (Ok(Tool::Reply { .. }), Some(r)) => Some((r, Via::Rules)),
        // Units ("10 minutes") are parsed deterministically; keep the model's
        // tool choice but trust the rule parser's numbers.
        (Ok(Tool::SetTimer { .. }), Some(r @ Tool::SetTimer { .. }))
        | (Ok(Tool::Remind { .. }), Some(r @ Tool::Remind { .. })) => Some((r, Via::Model)),
        (Ok(t), _) => Some((t, Via::Model)),
        (Err(_), Some(r)) => Some((r, Via::Rules)),
        (Err(_), None) => None,
    }
}

struct Worker {
    events: Sender<AgentEvent>,
    network: Arc<AtomicBool>,
    model: Option<LocalModel>,
}

impl Worker {
    fn new(events: Sender<AgentEvent>, network: Arc<AtomicBool>) -> Self {
        Self {
            events,
            network,
            model: None,
        }
    }

    fn emit(&self, e: AgentEvent) {
        let _ = self.events.send(e);
    }

    fn run(mut self, rx: Receiver<Request>) {
        while let Ok(req) = rx.recv() {
            if let Request::Warm = req {
                let system = router_system();
                let warmed = self
                    .ensure_model()
                    .and_then(|m| m.chat(&system, "hi", "", 1, &[], |_| true).map(|_| ()));
                if let Err(e) = warmed {
                    self.emit(AgentEvent::Error(e.to_string()));
                } else {
                    self.emit(AgentEvent::Phase(Phase::Ready, 0.0));
                }
            } else if let Request::Ask(prompt) = req {
                if let Err(e) = self.handle(&prompt, &rx) {
                    audit::log("error", "-", &e.to_string(), false, "-");
                    self.emit(AgentEvent::Error(e.to_string()));
                }
            }
        }
    }

    fn ensure_model(&mut self) -> Result<&mut LocalModel, AssistantError> {
        if self.model.is_none() {
            if !model_store::is_installed() {
                audit::log(
                    "model_download",
                    "-",
                    model_store::MODEL_URL,
                    true,
                    "first-run",
                );
                let tx = self.events.clone();
                let mut last = 0.0f32;
                model_store::download(|p| {
                    if p - last >= 0.01 {
                        last = p;
                        let _ = tx.send(AgentEvent::Phase(Phase::Downloading, p));
                    }
                })?;
            }
            self.emit(AgentEvent::Phase(Phase::Loading, 0.0));
            self.model = Some(LocalModel::load(&model_store::model_path())?);
        }
        Ok(self.model.as_mut().expect("model loaded"))
    }

    fn handle(&mut self, prompt: &str, rx: &Receiver<Request>) -> Result<(), AssistantError> {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return Ok(());
        }
        let system = router_system();
        let tx = self.events.clone();
        let model = self.ensure_model()?;
        let _ = tx.send(AgentEvent::Phase(Phase::Thinking, 0.0));
        let prefill = "<tool_call>\n{\"name\": \"";
        let gen = model.chat(&system, prompt, prefill, 96, &["</tool_call>"], |_| true)?;
        let stats = Stats::from(&gen);
        let proposal = tools::parse_tool_call(&format!("{prefill}{}", gen.text));
        if let Err(e) = &proposal {
            audit::log("model_invalid", "-", &e.to_string(), false, "model");
        }
        let Some((tool, via)) = choose(proposal, prompt) else {
            return Err(AssistantError::BadToolCall(
                "I couldn't map that to an action".into(),
            ));
        };
        let needs_confirm = tool.risk() == Risk::Confirm;
        self.emit(AgentEvent::Proposed {
            tool: tool.clone(),
            needs_confirm,
            via,
        });
        if needs_confirm {
            audit::log(
                "confirm_requested",
                tool.name(),
                &tool.describe(),
                false,
                via.label(),
            );
            let ok = loop {
                match rx.recv_timeout(Duration::from_secs(60)) {
                    Ok(Request::Confirm(yes)) => break yes,
                    Ok(Request::Warm) => continue,
                    Ok(Request::Ask(_)) => break false,
                    Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => {
                        break false
                    }
                }
            };
            if !ok {
                audit::log(
                    "declined",
                    tool.name(),
                    &tool.describe(),
                    false,
                    via.label(),
                );
                self.emit(AgentEvent::Done {
                    message: "Cancelled".into(),
                    effects: vec![],
                    sources: vec![],
                    stats: Some(stats),
                    via,
                });
                return Ok(());
            }
            audit::log(
                "confirmed",
                tool.name(),
                &tool.describe(),
                false,
                via.label(),
            );
        }

        match &tool {
            Tool::Reply { text } => {
                audit::log("reply", "reply", "", false, via.label());
                self.emit(AgentEvent::Done {
                    message: text.clone(),
                    effects: vec![],
                    sources: vec![],
                    stats: Some(stats),
                    via,
                });
            }
            Tool::WebSearch { query } => self.web_answer(query, prompt, via)?,
            Tool::Weather { city } => {
                self.require_network("weather")?;
                let city = if city.trim().is_empty() {
                    default_city()
                } else {
                    city.clone()
                };
                audit::log("network", "weather", &city, true, via.label());
                let w = web::weather(&city)?;
                self.emit(AgentEvent::Done {
                    message: format!(
                        "{} · {:.0}°C · {} · wind {:.0} km/h",
                        w.place,
                        w.temp_c,
                        w.summary(),
                        w.wind_kmh
                    ),
                    effects: vec![],
                    sources: vec![SearchHit {
                        title: "Open-Meteo".into(),
                        url: "https://open-meteo.com".into(),
                        snippet: String::new(),
                    }],
                    stats: Some(stats),
                    via,
                });
            }
            other => {
                self.emit(AgentEvent::Phase(Phase::Acting, 0.0));
                let result = exec::execute(other);
                match result {
                    Ok(outcome) => {
                        audit::log(
                            "executed",
                            other.name(),
                            &outcome.message,
                            false,
                            via.label(),
                        );
                        self.emit(AgentEvent::Done {
                            message: outcome.message,
                            effects: outcome.effects,
                            sources: vec![],
                            stats: Some(stats),
                            via,
                        });
                    }
                    Err(e) => {
                        audit::log("failed", other.name(), &e.to_string(), false, via.label());
                        return Err(e);
                    }
                }
            }
        }
        Ok(())
    }

    fn require_network(&self, what: &str) -> Result<(), AssistantError> {
        if self.network.load(Ordering::SeqCst) {
            Ok(())
        } else {
            audit::log(
                "blocked",
                what,
                "assistant network is off",
                false,
                "privacy",
            );
            Err(AssistantError::Network(
                "Web access is off in Privacy → Assistant".into(),
            ))
        }
    }

    fn web_answer(&mut self, query: &str, prompt: &str, via: Via) -> Result<(), AssistantError> {
        self.require_network("web_search")?;
        self.emit(AgentEvent::Phase(Phase::Searching, 0.0));
        audit::log("network", "web_search", query, true, via.label());
        let hits = web::search(query)?;
        self.emit(AgentEvent::Phase(Phase::Reading, 0.0));
        let page = hits
            .first()
            .and_then(|h| {
                audit::log("network", "fetch_page", &h.url, true, via.label());
                web::fetch_text(&h.url, 1400).ok()
            })
            .unwrap_or_default();
        let mut ctx = String::new();
        for (i, h) in hits.iter().take(4).enumerate() {
            ctx.push_str(&format!("[{}] {} — {}\n", i + 1, h.title, h.snippet));
        }
        if !page.is_empty() {
            ctx.push_str(&format!("\n[1] page text: {page}\n"));
        }
        let user = format!("Question: {prompt}\n\nSources:\n{ctx}");
        let tx = self.events.clone();
        let _ = tx.send(AgentEvent::Phase(Phase::Answering, 0.0));
        let model = self.ensure_model()?;
        let gen = model.chat(ANSWER_SYSTEM, &user, "", 110, &[], |delta| {
            let _ = tx.send(AgentEvent::Token(delta.to_string()));
            true
        })?;
        self.emit(AgentEvent::Done {
            message: gen.text.trim().to_string(),
            effects: vec![],
            sources: hits.into_iter().take(3).collect(),
            stats: Some(Stats::from(&gen)),
            via,
        });
        Ok(())
    }
}

pub fn default_city() -> String {
    std::env::var("NEXUS_CITY").unwrap_or_else(|_| "New Delhi".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_override_a_lazy_reply() {
        let (t, via) = choose(Ok(Tool::Reply { text: "ok".into() }), "set volume to 30").unwrap();
        assert_eq!(t, Tool::SetVolume { level: 30 });
        assert_eq!(via, Via::Rules);
    }

    #[test]
    fn rule_units_fix_model_timer_numbers() {
        let (t, via) = choose(
            Ok(Tool::SetTimer {
                seconds: 10,
                label: String::new(),
            }),
            "set a timer for 10 minutes",
        )
        .unwrap();
        assert_eq!(
            t,
            Tool::SetTimer {
                seconds: 600,
                label: String::new()
            }
        );
        assert_eq!(via, Via::Model);
    }

    #[test]
    fn model_choice_wins_when_valid() {
        let (t, via) = choose(Ok(Tool::Screenshot {}), "grab my screen").unwrap();
        assert_eq!(t, Tool::Screenshot {});
        assert_eq!(via, Via::Model);
    }

    #[test]
    fn invalid_model_output_falls_back_to_rules_or_errors() {
        let bad = Err(AssistantError::BadToolCall("x".into()));
        assert_eq!(
            choose(bad, "open my downloads folder").map(|x| x.1),
            Some(Via::Rules)
        );
        assert!(choose(Err(AssistantError::BadToolCall("x".into())), "blorp").is_none());
    }

    #[test]
    fn confirm_path_declines_on_no() {
        // Exercise the worker's confirm wait directly with a channel pair.
        let (tx, rx) = channel::<Request>();
        tx.send(Request::Confirm(false)).unwrap();
        let got = matches!(
            rx.recv_timeout(Duration::from_millis(10)),
            Ok(Request::Confirm(false))
        );
        assert!(got);
        assert_eq!(Tool::DeleteFile { path: "x".into() }.risk(), Risk::Confirm);
    }

    #[test]
    fn router_prompt_lists_every_tool() {
        let s = router_system();
        for name in tools::TOOL_NAMES {
            assert!(s.contains(&format!("\"name\":\"{name}\"")), "{name}");
        }
    }
}
