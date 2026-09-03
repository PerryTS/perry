//! Cancellable child-process timeout waiters.

use super::*;
use std::sync::mpsc::{self, RecvTimeoutError};

/// Spawn the process waiter and, when configured, a timeout waiter. The
/// process waiter owns the sole cancellation sender, so a completed child
/// wakes its timeout waiter instead of leaving an OS thread asleep until the
/// original deadline.
pub(super) fn cp_spawn_waiter(handle: u64, waiter: CpWaiter, timeout: Option<(Duration, i32)>) {
    let timeout_cancel = timeout.map(|(timeout, signal)| {
        let (cancel, cancelled) = mpsc::channel();
        std::thread::spawn(move || {
            if matches!(
                cancelled.recv_timeout(timeout),
                Err(RecvTimeoutError::Timeout)
            ) {
                cp_push_event(CpEvent::Timeout { handle, signal });
            }
        });
        cancel
    });

    std::thread::spawn(move || {
        let (code, signal) = waiter();
        if let Some(cancel) = timeout_cancel {
            let _ = cancel.send(());
        }
        cp_push_event(CpEvent::Exited {
            handle,
            code,
            signal,
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::time::Instant;

    #[test]
    fn completed_children_wake_fifty_long_timeout_waiters() {
        const CHILDREN: usize = 50;
        let ready = Arc::new(Barrier::new(CHILDREN + 1));
        let mut cancels = Vec::with_capacity(CHILDREN);
        let mut completions = Vec::with_capacity(CHILDREN);

        for _ in 0..CHILDREN {
            let (cancel, cancelled) = mpsc::channel();
            let (completed, completion) = mpsc::channel();
            let ready = Arc::clone(&ready);
            std::thread::spawn(move || {
                ready.wait();
                let timed_out = matches!(
                    cancelled.recv_timeout(Duration::from_secs(60)),
                    Err(RecvTimeoutError::Timeout)
                );
                let _ = completed.send(timed_out);
            });
            cancels.push(cancel);
            completions.push(completion);
        }

        ready.wait();
        let started = Instant::now();
        for cancel in cancels {
            cancel.send(()).expect("timeout waiter should be alive");
        }
        for completion in completions {
            assert_eq!(
                completion.recv_timeout(Duration::from_secs(1)),
                Ok(false),
                "timeout waiter did not stop after child completion"
            );
        }
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn elapsed_deadline_still_selects_the_timeout_arm() {
        let (_cancel, cancelled) = mpsc::channel::<()>();
        assert!(matches!(
            cancelled.recv_timeout(Duration::from_millis(10)),
            Err(RecvTimeoutError::Timeout)
        ));
    }
}
