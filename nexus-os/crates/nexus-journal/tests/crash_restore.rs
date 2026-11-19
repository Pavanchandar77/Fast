//! Any sequence of journaled mutations, then a simulated crash, then a
//! restart, restores the FakeSystem to the snapshot from before the sequence.

use nexus_journal::{FakeSystem, Journal, Mutation, PriorityClass, Rect, SystemMutator};
use proptest::prelude::*;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Debug)]
enum Step {
    Priority {
        pid: u32,
        class: PriorityClass,
    },
    Job {
        pid: u32,
        percent: u32,
    },
    Suspend {
        pid: u32,
    },
    TaskbarHide,
    AppBar {
        hwnd: u64,
        edge: u32,
    },
    Timer {
        value: u32,
    },
    Cloak {
        hwnd: u64,
    },
    Hide {
        hwnd: u64,
    },
    Move {
        hwnd: u64,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    },
}

fn arb_priority() -> impl Strategy<Value = PriorityClass> {
    prop_oneof![
        Just(PriorityClass::Idle),
        Just(PriorityClass::BelowNormal),
        Just(PriorityClass::Normal),
        Just(PriorityClass::AboveNormal),
        Just(PriorityClass::High),
        Just(PriorityClass::Realtime),
    ]
}

fn arb_step() -> impl Strategy<Value = Step> {
    fn pid() -> impl Strategy<Value = u32> {
        1u32..=3
    }
    fn hwnd() -> impl Strategy<Value = u64> {
        1u64..=3
    }
    prop_oneof![
        (pid(), arb_priority()).prop_map(|(pid, class)| Step::Priority { pid, class }),
        (pid(), prop_oneof![Just(0u32), Just(1), Just(5), Just(10)])
            .prop_map(|(pid, percent)| Step::Job { pid, percent }),
        pid().prop_map(|pid| Step::Suspend { pid }),
        Just(Step::TaskbarHide),
        (hwnd(), 0u32..=3).prop_map(|(hwnd, edge)| Step::AppBar { hwnd, edge }),
        prop_oneof![Just(5_000u32), Just(15_625u32), Just(156_250u32)]
            .prop_map(|value| Step::Timer { value }),
        hwnd().prop_map(|hwnd| Step::Cloak { hwnd }),
        hwnd().prop_map(|hwnd| Step::Hide { hwnd }),
        (hwnd(), -20i32..400, -20i32..400, 20i32..800, 20i32..600)
            .prop_map(|(hwnd, x, y, w, h)| Step::Move { hwnd, x, y, w, h }),
    ]
}

fn mutation_from(system: &FakeSystem, step: &Step) -> Option<Mutation> {
    match *step {
        Step::Priority { pid, class } => {
            let previous = system.priority_of(pid);
            if previous == class {
                None
            } else {
                Some(Mutation::ProcessPriority {
                    pid,
                    previous,
                    new: class,
                })
            }
        }
        Step::Job { pid, percent } => {
            let previous_percent = system.job_caps.get(&pid).copied();
            let already = match previous_percent {
                None => percent == 0,
                Some(p) => p == percent,
            };
            if already {
                None
            } else {
                Some(Mutation::JobCap {
                    pid,
                    previous_percent,
                    new_percent: percent,
                })
            }
        }
        Step::Suspend { pid } => {
            if system.suspended.contains(&pid) {
                None
            } else {
                Some(Mutation::Suspend { pid })
            }
        }
        Step::TaskbarHide => {
            if system.taskbar_hidden {
                None
            } else {
                Some(Mutation::TaskbarHide)
            }
        }
        Step::AppBar { hwnd, edge } => {
            if system.appbars.contains(&(hwnd, edge)) {
                None
            } else {
                Some(Mutation::AppBar {
                    hwnd,
                    edge,
                    rect: Rect {
                        x: 0,
                        y: 0,
                        width: 1920,
                        height: 28,
                    },
                })
            }
        }
        Step::Timer { value } => {
            if system.timer_100ns == value {
                None
            } else {
                Some(Mutation::TimerResolution {
                    previous_100ns: system.timer_100ns,
                    new_100ns: value,
                })
            }
        }
        Step::Cloak { hwnd } => {
            if system.cloaked.contains(&hwnd) {
                None
            } else {
                Some(Mutation::WindowCloak { hwnd })
            }
        }
        Step::Hide { hwnd } => {
            if system.hidden.contains(&hwnd) {
                None
            } else {
                Some(Mutation::WindowHide { hwnd })
            }
        }
        Step::Move { hwnd, x, y, w, h } => {
            let new = Rect {
                x,
                y,
                width: w,
                height: h,
            };
            let previous = system.position_of(hwnd);
            if previous == new {
                None
            } else {
                Some(Mutation::WindowPosition {
                    hwnd,
                    previous,
                    new,
                })
            }
        }
    }
}

fn scratch_dir() -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "nxjr-prop-{}-{}-{n}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("prop scratch");
    dir
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn crash_then_restart_restores_original(
        steps in prop::collection::vec(arb_step(), 0..24),
        drop_last_apply in proptest::bool::ANY,
    ) {
        let dir = scratch_dir();
        let path = dir.join("state.nxjr");
        let mut system = FakeSystem::clean();
        let original = system.clone();
        {
            let mut journal = Journal::open(&path).expect("open");
            // Build a coherent sequence against a scratch copy so each
            // mutation's `previous` is the live value at record time.
            let mut scratch = system.clone();
            let mut pending = Vec::new();
            for step in &steps {
                if let Some(mutation) = mutation_from(&scratch, step) {
                    scratch.apply(&mutation).expect("preview");
                    pending.push(mutation);
                }
            }
            let last = pending.len();
            for (i, mutation) in pending.iter().enumerate() {
                journal.record(mutation).expect("record");
                let skip_apply = drop_last_apply && i + 1 == last;
                if !skip_apply {
                    system.apply(mutation).expect("apply");
                }
            }
            // Crash: drop the journal without restore. The file stays on disk.
        }
        {
            let mut journal = Journal::open(&path).expect("reopen");
            journal.restore_all(&mut system).expect("restore");
            prop_assert!(journal.is_empty());
        }
        prop_assert_eq!(system, original);
        let _ = fs::remove_dir_all(&dir);
    }
}
