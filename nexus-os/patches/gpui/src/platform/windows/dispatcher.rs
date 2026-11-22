use std::{
    thread::{ThreadId, current},
    time::Duration,
};

use async_task::Runnable;
use flume::Sender;
use util::ResultExt;
use windows::{
    System::Threading::{
        ThreadPool, ThreadPoolTimer, TimerElapsedHandler, WorkItemHandler, WorkItemPriority,
    },
    Win32::{
        Foundation::{LPARAM, WPARAM},
        UI::WindowsAndMessaging::PostMessageW,
    },
};

use crate::{
    HWND, PlatformDispatcher, SafeHwnd, TaskLabel, WM_GPUI_TASK_DISPATCHED_ON_MAIN_THREAD,
};

pub(crate) struct WindowsDispatcher {
    main_sender: Sender<Runnable>,
    main_thread_id: ThreadId,
    platform_window_handle: SafeHwnd,
    validation_number: usize,
}

impl WindowsDispatcher {
    pub(crate) fn new(
        main_sender: Sender<Runnable>,
        platform_window_handle: HWND,
        validation_number: usize,
    ) -> Self {
        let main_thread_id = current().id();
        let platform_window_handle = platform_window_handle.into();

        WindowsDispatcher {
            main_sender,
            main_thread_id,
            platform_window_handle,
            validation_number,
        }
    }

    // Nexus: if the WinRT thread pool is unavailable (CreateTimer/RunAsync fail,
    // e.g. a stripped-down Windows or wine), the runnable used to be dropped,
    // which cancels the task and makes whoever awaits it panic with "Task
    // polled after completion" inside the window procedure (abort, silent exit).
    // Fall back to a plain thread instead.
    fn dispatch_on_threadpool(&self, runnable: Runnable) {
        let slot = std::sync::Arc::new(std::sync::Mutex::new(Some(runnable)));
        let handler = {
            let slot = slot.clone();
            WorkItemHandler::new(move |_| {
                if let Some(r) = slot.lock().ok().and_then(|mut s| s.take()) {
                    r.run();
                }
                Ok(())
            })
        };
        if let Err(err) = ThreadPool::RunWithPriorityAsync(&handler, WorkItemPriority::High) {
            warn_fallback_once(&err);
            if let Some(r) = slot.lock().ok().and_then(|mut s| s.take()) {
                std::thread::spawn(move || r.run());
            }
        }
    }

    fn dispatch_on_threadpool_after(&self, runnable: Runnable, duration: Duration) {
        let slot = std::sync::Arc::new(std::sync::Mutex::new(Some(runnable)));
        let handler = {
            let slot = slot.clone();
            TimerElapsedHandler::new(move |_| {
                if let Some(r) = slot.lock().ok().and_then(|mut s| s.take()) {
                    r.run();
                }
                Ok(())
            })
        };
        if let Err(err) = ThreadPoolTimer::CreateTimer(&handler, duration.into()) {
            warn_fallback_once(&err);
            if let Some(r) = slot.lock().ok().and_then(|mut s| s.take()) {
                std::thread::spawn(move || {
                    std::thread::sleep(duration);
                    r.run();
                });
            }
        }
    }
}

fn warn_fallback_once(err: &windows::core::Error) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        log::warn!("WinRT thread pool unavailable ({err:?}); using std threads for GPUI tasks");
    });
}

impl PlatformDispatcher for WindowsDispatcher {
    fn is_main_thread(&self) -> bool {
        current().id() == self.main_thread_id
    }

    fn dispatch(&self, runnable: Runnable, label: Option<TaskLabel>) {
        self.dispatch_on_threadpool(runnable);
        if let Some(label) = label {
            log::debug!("TaskLabel: {label:?}");
        }
    }

    fn dispatch_on_main_thread(&self, runnable: Runnable) {
        match self.main_sender.send(runnable) {
            Ok(_) => unsafe {
                PostMessageW(
                    Some(self.platform_window_handle.as_raw()),
                    WM_GPUI_TASK_DISPATCHED_ON_MAIN_THREAD,
                    WPARAM(self.validation_number),
                    LPARAM(0),
                )
                .log_err();
            },
            Err(runnable) => {
                // NOTE: Runnable may wrap a Future that is !Send.
                //
                // This is usually safe because we only poll it on the main thread.
                // However if the send fails, we know that:
                // 1. main_receiver has been dropped (which implies the app is shutting down)
                // 2. we are on a background thread.
                // It is not safe to drop something !Send on the wrong thread, and
                // the app will exit soon anyway, so we must forget the runnable.
                std::mem::forget(runnable);
            }
        }
    }

    fn dispatch_after(&self, duration: Duration, runnable: Runnable) {
        self.dispatch_on_threadpool_after(runnable, duration);
    }
}
