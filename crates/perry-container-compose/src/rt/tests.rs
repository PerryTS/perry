//! The runtime's own contract: every leaf actually waits on the loop, and a
//! dropped child is terminated rather than orphaned.

use super::*;
use std::time::{Duration, Instant};

#[test]
fn block_on_returns_a_ready_value() {
    assert_eq!(block_on(async { 7 }), 7);
    assert!(!in_context());
    assert!(block_on(async { in_context() }));
}

#[test]
fn sleep_waits_at_least_its_duration() {
    let started = Instant::now();
    block_on(sleep(Duration::from_millis(60)));
    assert!(started.elapsed() >= Duration::from_millis(60));
}

#[test]
fn timeout_passes_a_fast_future_through() {
    let out = block_on(timeout(Duration::from_secs(5), async {
        sleep(Duration::from_millis(5)).await;
        "done"
    }));
    assert_eq!(out, Ok("done"));
}

#[test]
fn timeout_fires_on_a_slow_future() {
    let started = Instant::now();
    let out = block_on(timeout(
        Duration::from_millis(50),
        sleep(Duration::from_secs(30)),
    ));
    assert!(out.is_err());
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn nested_block_on_gets_its_own_loop_and_restores_the_outer_one() {
    let out = block_on(async {
        sleep(Duration::from_millis(5)).await;
        let inner = block_on(async {
            sleep(Duration::from_millis(5)).await;
            1
        });
        // The outer loop is current again: its leaves still work.
        sleep(Duration::from_millis(5)).await;
        inner + 1
    });
    assert_eq!(out, 2);
}

#[test]
fn a_waker_fired_from_another_thread_wakes_the_loop() {
    let lock = std::sync::Arc::new(Mutex::new(0u32));
    let held = block_on(lock.lock_arc());
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        drop(held);
    });
    let started = Instant::now();
    // Nothing on this loop can complete by itself: only the other thread's
    // unlock (an async-lock waker) can end this wait.
    let value = block_on(async { *lock.lock().await + 1 });
    releaser.join().unwrap();
    assert_eq!(value, 1);
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
#[should_panic(expected = "outside rt::block_on")]
fn a_leaf_polled_outside_block_on_panics() {
    let mut fut = std::pin::pin!(sleep(Duration::from_secs(1)));
    let waker = Waker::noop();
    let _ = fut.as_mut().poll(&mut Context::from_waker(waker));
}

#[cfg(unix)]
mod unix {
    use super::*;

    #[test]
    fn output_captures_both_streams_and_the_exit_code() {
        let out = block_on(
            Command::new("/bin/sh")
                .args(["-c", "printf out; printf err 1>&2; exit 3"])
                .output(),
        )
        .unwrap();
        assert_eq!(out.stdout, b"out");
        assert_eq!(out.stderr, b"err");
        assert_eq!(out.status.code(), Some(3));
        assert!(!out.status.success());
        assert_eq!(out.status.to_string(), "exit status: 3");
    }

    #[test]
    fn output_reads_large_streams_to_eof() {
        // Larger than any pipe buffer on both streams at once: a reader that
        // only drained one pipe, or stopped at exit, would deadlock or truncate.
        let out = block_on(
            Command::new("/bin/sh")
                .args([
                    "-c",
                    "head -c 300000 /dev/zero; head -c 200000 /dev/zero 1>&2",
                ])
                .output(),
        )
        .unwrap();
        assert!(out.status.success());
        assert_eq!(out.stdout.len(), 300_000);
        assert_eq!(out.stderr.len(), 200_000);
    }

    #[test]
    fn stdin_is_null_for_output() {
        let out = block_on(Command::new("/bin/cat").output()).unwrap();
        assert!(out.status.success());
        assert!(out.stdout.is_empty());
    }

    #[test]
    fn a_missing_program_is_a_spawn_error_or_exit_127() {
        match block_on(Command::new("perry-rt-no-such-program-xyz").output()) {
            Err(e) => assert_eq!(e.kind(), io::ErrorKind::NotFound, "{e}"),
            Ok(out) => assert_eq!(out.status.code(), Some(127)),
        }
    }

    #[test]
    fn status_reports_the_exit_code() {
        let status = block_on(Command::new("/bin/sh").args(["-c", "exit 0"]).status()).unwrap();
        assert!(status.success());
    }

    #[test]
    fn a_timed_out_child_is_terminated_not_orphaned() {
        let dir = std::env::temp_dir().join(format!("perry-rt-kill-{}", std::process::id()));
        let _ = std::fs::remove_file(&dir);
        // Writes its pid, then would sleep for a minute.
        let script = format!("echo $$ > '{}'; exec sleep 60", dir.display());
        let started = Instant::now();
        let out = block_on(timeout(
            Duration::from_millis(300),
            Command::new("/bin/sh").args(["-c", &script]).output(),
        ));
        assert!(out.is_err(), "the timeout must fire");
        assert!(started.elapsed() < Duration::from_secs(10));
        let pid: i32 = std::fs::read_to_string(&dir)
            .expect("child wrote its pid")
            .trim()
            .parse()
            .unwrap();
        let _ = std::fs::remove_file(&dir);
        // The loop has been dropped, so the child must be gone. A zombie
        // (`Z`) counts as gone: it is no longer running.
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let stat = std::process::Command::new("ps")
                .args(["-o", "stat=", "-p", &pid.to_string()])
                .output()
                .unwrap();
            let stat = String::from_utf8_lossy(&stat.stdout).trim().to_string();
            let alive = !stat.is_empty() && !stat.starts_with('Z');
            if !alive {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "child {pid} survived its timeout"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
