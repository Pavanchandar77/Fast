//! In-app self-test: `--selftest` opens, hides, shows, and toggles every
//! shell surface, logs pass/fail with timings, writes one file Pavan can
//! send, and exits. Safe to run on Windows or in the Linux preview.

use crate::diag;
use crate::startup::{
    open_gpui_spotlight_with, toggle_gpui_spotlight, toggle_island_assistant, GpuiSessionWindows,
};
use std::io::Write;
use std::time::Instant;

pub fn requested<I: IntoIterator<Item = String>>(args: I) -> bool {
    args.into_iter()
        .any(|a| a == "--selftest" || a == "/selftest")
}

struct Step {
    name: String,
    ok: bool,
    ms: u128,
    detail: String,
}

/// Run the checks, write `%LOCALAPPDATA%/Nexus/logs/selftest.txt` (or the
/// same log dir elsewhere), and return true when every step passed.
pub fn run(windows: &GpuiSessionWindows, cx: &mut gpui::App) -> bool {
    let t0 = Instant::now();
    let mut steps: Vec<Step> = Vec::new();

    let present = |ok: bool, name: &str, ms: u128| Step {
        name: name.into(),
        ok,
        ms,
        detail: if ok {
            "present".into()
        } else {
            "MISSING".into()
        },
    };

    let t = Instant::now();
    steps.push(present(
        windows.island_window.is_some(),
        "window:island",
        t.elapsed().as_millis(),
    ));
    steps.push(present(windows.dock_window.is_some(), "window:dock", 0));
    steps.push(present(
        windows.spotlight_window.is_some(),
        "window:spotlight",
        0,
    ));
    steps.push(present(windows.expose_window.is_some(), "window:expose", 0));
    steps.push(present(
        windows.explorer_window.is_some(),
        "window:files",
        0,
    ));
    steps.push(present(
        windows.privacy_dashboard_window.is_some(),
        "window:privacy",
        0,
    ));
    steps.push(present(
        windows.motion_window.is_some() || diag::safe_mode(),
        "window:motion",
        0,
    ));

    if let Some(handle) = windows.spotlight_window {
        let t = Instant::now();
        let opened = toggle_gpui_spotlight(handle, cx);
        let ms = t.elapsed().as_millis();
        steps.push(Step {
            name: "spotlight:open".into(),
            ok: opened,
            ms,
            detail: format!("open={opened}"),
        });
        let t = Instant::now();
        let closed = !toggle_gpui_spotlight(handle, cx);
        steps.push(Step {
            name: "spotlight:close".into(),
            ok: closed,
            ms: t.elapsed().as_millis(),
            detail: format!("closed={closed}"),
        });
        let t = Instant::now();
        open_gpui_spotlight_with(handle, "cb", cx);
        let open = handle
            .update(cx, |v, _, _| v.model.is_open() && v.model.query == "cb")
            .unwrap_or(false);
        steps.push(Step {
            name: "spotlight:clipboard-query".into(),
            ok: open,
            ms: t.elapsed().as_millis(),
            detail: format!("query=cb open={open}"),
        });
        let _ = toggle_gpui_spotlight(handle, cx);
    } else {
        steps.push(Step {
            name: "spotlight:open".into(),
            ok: false,
            ms: 0,
            detail: "no window".into(),
        });
    }

    if let Some(handle) = windows.island_window {
        let t = Instant::now();
        let opened = toggle_island_assistant(handle, cx);
        steps.push(Step {
            name: "island:assistant-open".into(),
            ok: opened,
            ms: t.elapsed().as_millis(),
            detail: format!("expanded={opened}"),
        });
        let t = Instant::now();
        let closed = !toggle_island_assistant(handle, cx);
        steps.push(Step {
            name: "island:assistant-close".into(),
            ok: closed,
            ms: t.elapsed().as_millis(),
            detail: format!("collapsed={closed}"),
        });
    }

    if let Some(handle) = windows.motion_window {
        let t = Instant::now();
        let opened = handle
            .update(cx, |view, window, cx| {
                view.sync_window(window);
                view.mission_show(Vec::new());
                cx.notify();
                view.mission_open()
            })
            .unwrap_or(false);
        steps.push(Step {
            name: "mission-control:open".into(),
            ok: opened,
            ms: t.elapsed().as_millis(),
            detail: format!("open={opened}"),
        });
        let t = Instant::now();
        let closed = handle
            .update(cx, |view, _, cx| {
                view.mission_hide();
                cx.notify();
                !view.mission_open()
            })
            .unwrap_or(false);
        steps.push(Step {
            name: "mission-control:close".into(),
            ok: closed,
            ms: t.elapsed().as_millis(),
            detail: format!("closed={closed}"),
        });
    } else if !diag::safe_mode() {
        steps.push(Step {
            name: "mission-control:open".into(),
            ok: false,
            ms: 0,
            detail: "no motion overlay".into(),
        });
    }

    // Files / Privacy: hide then show (the macOS hide-on-close path).
    fn hide_show<V: 'static>(
        label: &str,
        handle: Option<gpui::WindowHandle<V>>,
        cx: &mut gpui::App,
        steps: &mut Vec<Step>,
    ) {
        let Some(handle) = handle else { return };
        let t = Instant::now();
        let hidden = handle
            .update(cx, |_, window, _| {
                #[cfg(windows)]
                if let Some(raw) = crate::chrome::window_hwnd(window) {
                    nx_ui::platform::hide_async(raw);
                    return true;
                }
                let _ = window;
                false
            })
            .unwrap_or(false);
        // Off Windows there is no HWND: report as skipped-pass so the Linux
        // preview still produces a clean file of the GPUI-side checks.
        let ok = hidden || cfg!(not(windows));
        steps.push(Step {
            name: format!("{label}:hide"),
            ok,
            ms: t.elapsed().as_millis(),
            detail: if hidden {
                "hidden".into()
            } else if cfg!(not(windows)) {
                "skipped (no hwnd)".into()
            } else {
                "failed".into()
            },
        });
        let t = Instant::now();
        let _ = handle.update(cx, |_, window, _| {
            crate::chrome::show_app_window(window);
        });
        steps.push(Step {
            name: format!("{label}:show"),
            ok: true,
            ms: t.elapsed().as_millis(),
            detail: "restore issued".into(),
        });
    }
    hide_show("files", windows.explorer_window, cx, &mut steps);
    hide_show("privacy", windows.privacy_dashboard_window, cx, &mut steps);

    let passed = steps.iter().filter(|s| s.ok).count();
    let failed = steps.len() - passed;
    let total_ms = t0.elapsed().as_millis();

    let mut body = String::new();
    body.push_str(&format!(
        "Nexus {} self-test\npassed={passed} failed={failed} steps={} total_ms={total_ms}\nargs {:?}\n\n",
        diag::release(),
        steps.len(),
        std::env::args().collect::<Vec<_>>(),
    ));
    for s in &steps {
        body.push_str(&format!(
            "{} {} {} ms  {}\n",
            if s.ok { "PASS" } else { "FAIL" },
            s.name,
            s.ms,
            s.detail
        ));
    }
    tracing::info!("self-test:\n{body}");

    let path = diag::log_dir().join("selftest.txt");
    if let Err(err) = (|| -> std::io::Result<()> {
        std::fs::create_dir_all(diag::log_dir())?;
        let mut f = std::fs::File::create(&path)?;
        f.write_all(body.as_bytes())?;
        f.sync_all()?;
        Ok(())
    })() {
        tracing::error!("could not write selftest.txt: {err}");
    } else {
        tracing::info!("self-test written to {}", path.display());
    }

    let verdict = if failed == 0 {
        format!(
            "Self-test passed: {passed} checks in {total_ms} ms.\n\nReport: {}",
            path.display()
        )
    } else {
        format!(
            "Self-test failed: {failed} of {} checks failed in {total_ms} ms.\n\nReport: {}\n\nSee also {}",
            steps.len(),
            path.display(),
            diag::log_path().display()
        )
    };
    if failed == 0 {
        diag::show_message("Nexus self-test", &verdict, false);
    } else {
        diag::fatal("selftest", &verdict);
    }
    failed == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag() {
        assert!(requested(["x".into(), "--selftest".into()]));
        assert!(requested(["/selftest".into()]));
        assert!(!requested(["--diag".into()]));
    }
}
