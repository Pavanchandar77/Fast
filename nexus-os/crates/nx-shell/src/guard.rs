//! Panic fences: one widget's bug must not take the whole shell down. A
//! panic in a guarded render or poll is logged (nexus.log via the panic hook
//! and here) and that frame falls back to an empty element.

use gpui::{div, AnyElement, IntoElement};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU32, Ordering};

static PANICS: AtomicU32 = AtomicU32::new(0);

thread_local! {
    static DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// True while this thread is inside a [`guard`]: a panic here will be caught,
/// so crash reporting should not treat it as fatal.
pub fn in_guard() -> bool {
    DEPTH.with(|d| d.get() > 0)
}

/// How many guarded panics this session has absorbed.
pub fn panic_count() -> u32 {
    PANICS.load(Ordering::Relaxed)
}

/// Run `f`; on panic log it, count it and return `None`.
pub fn guard<T>(what: &str, f: impl FnOnce() -> T) -> Option<T> {
    DEPTH.with(|d| d.set(d.get() + 1));
    let result = catch_unwind(AssertUnwindSafe(f));
    DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    match result {
        Ok(v) => Some(v),
        Err(err) => {
            let n = PANICS.fetch_add(1, Ordering::Relaxed) + 1;
            let msg = err
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| err.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "non-string panic".into());
            tracing::error!("guard: {what} panicked ({msg}); fell back (panic #{n})");
            None
        }
    }
}

/// A guarded render: the widget draws nothing this frame if it panics.
pub fn render(what: &str, f: impl FnOnce() -> AnyElement) -> AnyElement {
    guard(what, f).unwrap_or_else(|| div().into_any_element())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panics_are_contained_and_counted() {
        let before = panic_count();
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let out: Option<u32> = guard("test", || panic!("boom"));
        std::panic::set_hook(prev);
        assert!(out.is_none());
        assert_eq!(guard("ok", || 7), Some(7));
        assert!(panic_count() > before);
    }
}
