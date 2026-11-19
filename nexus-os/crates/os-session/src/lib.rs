//! Session library: product startup orchestration for Nexus-OS.

pub mod startup;

pub use startup::{
    start_session, start_session_with, ComponentStatus, SessionComponent, SessionHandle,
};
