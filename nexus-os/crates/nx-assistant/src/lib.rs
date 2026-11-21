//! Nexus Assistant: the on-device "Siri" that lives in the Dynamic Island.
//!
//! - Local LLM: Qwen3-0.6B (Q4_K_M GGUF, Apache-2.0) run in-process on CPU via
//!   candle; downloaded once to `%LOCALAPPDATA%\Nexus\models`.
//! - Tools: strict JSON tool calls, validated and risk-classified; destructive
//!   actions need a confirm tap in the island; every action is audit-logged.
//! - Network: only `web_search`/`weather` (plus the one-time model download),
//!   gated by the Privacy "Assistant network" toggle.

pub mod agent;
pub mod audit;
pub mod exec;
pub mod eyes;
pub mod intent;
pub mod llm;
pub mod model_store;
pub mod tokenizer;
pub mod tools;
pub mod web;

pub use agent::{AgentEvent, Assistant, Phase, Stats, Via};
pub use exec::Effect;
pub use eyes::{EyeFrame, Eyes, Mood};
pub use tools::{Risk, Tool};

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum AssistantError {
    #[error("model: {0}")]
    Model(String),
    #[error("{0}")]
    BadToolCall(String),
    #[error("{0}")]
    Action(String),
    #[error("network: {0}")]
    Network(String),
}
