//! Integration tests for the ops engine (tempdirs).

use super::*;
use std::path::Path;
use std::time::Duration;

struct Env {
    _t: tempfile::TempDir,
    root: PathBuf,
    engine: OpEngine,
}

fn env() -> Env {
    let t = tempfile::tempdir().expect("tempdir");
    let root = t.path().to_path_buf();
    let trash = Arc::new(DirTrash::new(root.join(".trash")).expect("trash"));
    let mut cfg = EngineConfig::new(root.join(".nx/ops.journal"), trash);
    cfg.buffer_size = 4096;
    let (engine, _) = OpEngine::open(cfg).expect("engine");
    Env {
        _t: t,
        root,
        engine,
    }
}

fn write(p: &Path, s: &str) {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).expect("mkdir");
    }
    std::fs::write(p, s).expect("write");
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_else(|_| "<missing>".into())
}

/// Wait for the job's terminal event, answering conflicts with `answer`.
fn finish(
    e: &OpEngine,
    job: JobId,
    answer: Option<(Resolution, bool)>,
) -> (JobEvent, Vec<JobEvent>) {
    let mut seen = Vec::new();
    loop {
        let ev = e
            .events()
            .recv_timeout(Duration::from_secs(10))
            .expect("event");
        if ev.job() != job {
            continue;
        }
        if let JobEvent::Conflict { .. } = ev {
            let (r, all) = answer.unwrap_or((Resolution::Cancel, false));
            assert!(e.resolve(job, r, all));
        }
        if ev.is_terminal() {
            return (ev, seen);
        }
        seen.push(ev);
    }
}

#[test]
fn copy_tree_and_undo() {
    let env = env();
    let src = env.root.join("src");
    write(&src.join("a.txt"), "A");
    write(&src.join("deep/b.txt"), "B");
    let dst = env.root.join("dst");
    std::fs::create_dir(&dst).expect("mkdir");
    let job = env
        .engine
        .submit(OpRequest::copy(vec![src.clone()], &dst))
        .expect("submit");
    let (end, seen) = finish(&env.engine, job, None);
    let JobEvent::Finished { summary, .. } = end else {
        panic!("{end:?}")
    };
    assert_eq!(summary.items_done, 4);
    assert_eq!(summary.bytes_done, 2);
    assert!(summary.undoable);
    assert_eq!(read(&dst.join("src/deep/b.txt")), "B");
    assert!(seen
        .iter()
        .any(|e| matches!(e, JobEvent::IndexChanges { .. })));
    // No temp files left behind.
    assert!(std::fs::read_dir(dst.join("src"))
        .expect("ls")
        .flatten()
        .all(|e| !e.file_name().to_string_lossy().contains("nxpart")));
    let u = env.engine.undo().expect("undo").expect("job");
    finish(&env.engine, u, None);
    assert!(!dst.join("src").exists());
    assert_eq!(read(&src.join("a.txt")), "A", "source untouched");
    assert!(
        env.engine.undo().expect("undo").is_none(),
        "nothing left to undo"
    );
}

#[test]
fn conflicts_skip_replace_keep_both() {
    let env = env();
    let a = env.root.join("a.txt");
    write(&a, "new");
    let dst = env.root.join("d");
    write(&dst.join("a.txt"), "old");

    let job = env
        .engine
        .submit(OpRequest::copy(vec![a.clone()], &dst))
        .expect("submit");
    let (end, seen) = finish(&env.engine, job, Some((Resolution::Skip, false)));
    assert!(seen.iter().any(|e| matches!(e, JobEvent::Conflict { .. })));
    assert!(matches!(end, JobEvent::Finished { ref summary, .. } if summary.skipped == 1));
    assert_eq!(read(&dst.join("a.txt")), "old");

    let job = env
        .engine
        .submit(OpRequest::copy(vec![a.clone()], &dst).with_conflict(ConflictPolicy::KeepBoth))
        .expect("submit");
    finish(&env.engine, job, None);
    assert_eq!(read(&dst.join("a (2).txt")), "new");

    let job = env
        .engine
        .submit(OpRequest::copy(vec![a.clone()], &dst))
        .expect("submit");
    finish(&env.engine, job, Some((Resolution::Replace, true)));
    assert_eq!(read(&dst.join("a.txt")), "new");
    // Undo restores the replaced original.
    let u = env.engine.undo().expect("undo").expect("job");
    finish(&env.engine, u, None);
    assert_eq!(read(&dst.join("a.txt")), "old");
    assert!(std::fs::read_dir(&dst)
        .expect("ls")
        .flatten()
        .all(|e| !e.file_name().to_string_lossy().contains("nxold")));

    let job = env
        .engine
        .submit(OpRequest::copy(vec![a.clone()], &dst))
        .expect("submit");
    let (end, _) = finish(&env.engine, job, Some((Resolution::Cancel, false)));
    assert!(matches!(end, JobEvent::Cancelled { .. }));
}

#[test]
fn move_rename_merge_and_undo() {
    let env = env();
    let src = env.root.join("photos");
    write(&src.join("1.jpg"), "1");
    write(&src.join("sub/2.jpg"), "2");
    let dst = env.root.join("archive");
    write(&dst.join("photos/0.jpg"), "0");
    let job = env
        .engine
        .submit(OpRequest::move_to(vec![src.clone()], &dst))
        .expect("submit");
    let (end, _) = finish(&env.engine, job, None);
    assert!(matches!(end, JobEvent::Finished { .. }), "{end:?}");
    assert_eq!(read(&dst.join("photos/1.jpg")), "1");
    assert_eq!(read(&dst.join("photos/sub/2.jpg")), "2");
    assert_eq!(read(&dst.join("photos/0.jpg")), "0");
    assert!(!src.exists(), "merged source removed");
    let u = env.engine.undo().expect("undo").expect("job");
    finish(&env.engine, u, None);
    assert_eq!(read(&src.join("1.jpg")), "1");
    assert_eq!(read(&src.join("sub/2.jpg")), "2");
    assert!(!dst.join("photos/1.jpg").exists());

    let f = env.root.join("old name.txt");
    write(&f, "x");
    let job = env
        .engine
        .submit(OpRequest {
            kind: OpKind::Rename {
                new_name: "new name.txt".into(),
            },
            sources: vec![f.clone()],
            conflict: ConflictPolicy::Ask,
        })
        .expect("submit");
    finish(&env.engine, job, None);
    assert_eq!(read(&env.root.join("new name.txt")), "x");
    let bad = env
        .engine
        .submit(OpRequest {
            kind: OpKind::Rename {
                new_name: "bad:name".into(),
            },
            sources: vec![env.root.join("new name.txt")],
            conflict: ConflictPolicy::Ask,
        })
        .expect("submit");
    let (end, _) = finish(&env.engine, bad, None);
    assert!(matches!(
        end,
        JobEvent::Failed {
            error: FsError::InvalidPath { .. },
            ..
        }
    ));
}

#[test]
fn into_itself_and_missing_items_fail_per_item() {
    let env = env();
    let d = env.root.join("dir");
    write(&d.join("x"), "x");
    let job = env
        .engine
        .submit(OpRequest::copy(
            vec![d.clone(), env.root.join("ghost")],
            d.join("x").parent().expect("p"),
        ))
        .expect("submit");
    let (end, seen) = finish(&env.engine, job, None);
    let fails: Vec<_> = seen
        .iter()
        .filter_map(|e| {
            if let JobEvent::ItemFailed { error, .. } = e {
                Some(error)
            } else {
                None
            }
        })
        .collect();
    assert!(fails
        .iter()
        .any(|e| matches!(e, FsError::IntoItself { .. })));
    assert!(fails.iter().any(|e| matches!(e, FsError::NotFound { .. })));
    assert!(matches!(end, JobEvent::Finished { ref summary, .. } if summary.failed == 2));
}

#[test]
fn trash_and_restore_via_undo() {
    let env = env();
    let f = env.root.join("victim.txt");
    write(&f, "precious");
    let job = env
        .engine
        .submit(OpRequest::delete(vec![f.clone()], true))
        .expect("submit");
    finish(&env.engine, job, None);
    assert!(!f.exists());
    let u = env.engine.undo().expect("undo").expect("job");
    finish(&env.engine, u, None);
    assert_eq!(read(&f), "precious");
    let job = env
        .engine
        .submit(OpRequest::delete(vec![f.clone()], false))
        .expect("submit");
    let (end, _) = finish(&env.engine, job, None);
    assert!(matches!(end, JobEvent::Finished { ref summary, .. } if !summary.undoable));
    assert!(!f.exists());
}

#[test]
fn pause_resume_cancel_large_copy() {
    let env = env();
    let big = env.root.join("big.bin");
    std::fs::write(&big, vec![7u8; 6 << 20]).expect("write");
    let dst = env.root.join("out");
    std::fs::create_dir(&dst).expect("mkdir");
    let job = env
        .engine
        .submit(OpRequest::copy(vec![big.clone()], &dst))
        .expect("submit");
    assert!(env.engine.pause(job));
    std::thread::sleep(Duration::from_millis(30));
    assert!(env.engine.resume(job));
    let (end, _) = finish(&env.engine, job, None);
    assert!(matches!(end, JobEvent::Finished { ref summary, .. } if summary.bytes_done == 6 << 20));
    assert_eq!(
        std::fs::metadata(dst.join("big.bin")).expect("md").len(),
        6 << 20
    );

    let dst2 = env.root.join("out2");
    std::fs::create_dir(&dst2).expect("mkdir");
    let job = env
        .engine
        .submit(OpRequest::copy(vec![big.clone()], &dst2))
        .expect("submit");
    env.engine.pause(job);
    env.engine.cancel(job);
    let (end, _) = finish(&env.engine, job, None);
    assert!(matches!(end, JobEvent::Cancelled { .. }), "{end:?}");
    assert!(
        std::fs::read_dir(&dst2).expect("ls").next().is_none(),
        "no partial file under any name"
    );
}

#[test]
fn new_folder_unique_and_duplicate_in_place() {
    let env = env();
    for _ in 0..2 {
        let job = env
            .engine
            .submit(OpRequest {
                kind: OpKind::NewFolder {
                    parent: env.root.clone(),
                    name: "untitled folder".into(),
                },
                sources: vec![],
                conflict: ConflictPolicy::Ask,
            })
            .expect("submit");
        finish(&env.engine, job, None);
    }
    assert!(env.root.join("untitled folder").is_dir());
    assert!(env.root.join("untitled folder (2)").is_dir());
    let f = env.root.join("doc.txt");
    write(&f, "d");
    let job = env
        .engine
        .submit(OpRequest::copy(vec![f.clone()], &env.root))
        .expect("submit");
    finish(&env.engine, job, None);
    assert_eq!(read(&env.root.join("doc (2).txt")), "d");
    assert!(env.engine.submit(OpRequest::delete(vec![], true)).is_err());
}

#[test]
fn undo_survives_restart() {
    let t = tempfile::tempdir().expect("tempdir");
    let root = t.path().to_path_buf();
    let journal = root.join("j/ops.journal");
    let trash: Arc<dyn Trash> = Arc::new(DirTrash::new(root.join(".trash")).expect("trash"));
    let f = root.join("f.txt");
    write(&f, "f");
    let d = root.join("d");
    std::fs::create_dir(&d).expect("mkdir");
    {
        let (e, _) = OpEngine::open(EngineConfig::new(&journal, Arc::clone(&trash))).expect("open");
        let job = e
            .submit(OpRequest::move_to(vec![f.clone()], &d))
            .expect("submit");
        finish(&e, job, None);
    }
    let (e, report) = OpEngine::open(EngineConfig::new(&journal, trash)).expect("reopen");
    assert!(report.interrupted_jobs.is_empty());
    assert_eq!(e.undo_label().as_deref(), Some("restored"));
    let u = e.undo().expect("undo").expect("job");
    finish(&e, u, None);
    assert_eq!(read(&f), "f");
}

#[cfg(unix)]
#[test]
fn copies_symlinks_as_links_and_survives_loops() {
    let env = env();
    let src = env.root.join("s");
    write(&src.join("real.txt"), "r");
    std::os::unix::fs::symlink(&src, src.join("loop")).expect("symlink");
    let dst = env.root.join("t");
    std::fs::create_dir(&dst).expect("mkdir");
    let job = env
        .engine
        .submit(OpRequest::copy(vec![src.clone()], &dst))
        .expect("submit");
    let (end, _) = finish(&env.engine, job, None);
    assert!(
        matches!(end, JobEvent::Finished { ref summary, .. } if summary.failed == 0),
        "{end:?}"
    );
    let l = dst.join("s/loop");
    assert!(std::fs::symlink_metadata(&l)
        .expect("md")
        .file_type()
        .is_symlink());
}

#[cfg(unix)]
#[test]
fn permission_denied_is_per_item() {
    use std::os::unix::fs::PermissionsExt;
    let env = env();
    let locked = env.root.join("locked");
    write(&locked.join("secret"), "s");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).expect("chmod");
    // Root ignores permissions; nothing to assert in that case.
    let readable = std::fs::read_dir(&locked).is_ok();
    let ok = env.root.join("ok.txt");
    write(&ok, "ok");
    let dst = env.root.join("dst");
    std::fs::create_dir(&dst).expect("mkdir");
    let job = env
        .engine
        .submit(OpRequest::copy(vec![locked.clone(), ok.clone()], &dst))
        .expect("submit");
    let (end, seen) = finish(&env.engine, job, None);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).expect("chmod back");
    assert_eq!(read(&dst.join("ok.txt")), "ok");
    if !readable {
        assert!(seen.iter().any(|e| matches!(
            e,
            JobEvent::ItemFailed {
                error: FsError::PermissionDenied { .. },
                ..
            }
        )));
        assert!(matches!(end, JobEvent::Finished { ref summary, .. } if summary.failed == 1));
    }
}
