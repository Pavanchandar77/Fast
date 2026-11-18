//! # nexus-model: Pure Platform-Independent Core Domain for Nexus-OS
//!
//! Contains all pure logic for Nexus-OS:
//! - High-precision interruptible damped harmonic oscillator spring physics solver (`spring`)
//! - Non-overlapping multi-layout tiling engine with gap verification (`layout`)
//! - Robust, case-insensitive hotkey string parser and action dispatch (`hotkeys`)
//! - Declarative configuration schema and bounds validator (`config`)
//! - Process priority, App Nap, and Job Object governor decision policy (`governor`)
//! - Deterministic workspace and window state machine (`state`)
//! - Abstract OS mutation interface and in-memory mock implementation (`platform`)
//!
//! Compiles on Linux and Windows with 0 platform dependencies.

pub mod config;
pub mod governor;
pub mod hotkeys;
pub mod layout;
pub mod platform;
pub mod spring;
pub mod state;

// Re-exports of primary types
pub use config::{ConfigValidationError, SystemConfig};
pub use governor::{
    ExecutionDecision, GovernorPolicy, GovernorPolicyConfig, MemoryDecision, PolicyDecision,
    ProcessPriorityTarget, ProcessSnapshot, ThrottleDecision, CRITICAL_PROCESSES,
};
pub use hotkeys::{Direction, Hotkey, HotkeyAction, HotkeyParseError, Key, Modifiers};
pub use layout::{LayoutEngine, LayoutKind, LayoutOptions, LayoutValidator, Rect, WindowId};
pub use platform::{
    AppBarEdge, FakePlatform, FakeProcessState, FakeWindowState, Platform, PlatformCall,
    PlatformError, ProcessPriority,
};
pub use spring::{Spring1D, Spring2D, SpringConfig, SpringRect, SpringSolver};
pub use state::{
    apply_mutations, ManagedWindowState, StateEvent, WindowManagerStateMachine, WindowMutation,
};
