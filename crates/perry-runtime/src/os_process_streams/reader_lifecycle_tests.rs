use super::{reader_slot_claim_start, reader_slot_claim_stop};
use std::sync::atomic::{AtomicBool, Ordering};

/// #10895: replays the interleaving that stranded fd 0 without a reader —
/// the reader decides to stop, and `resume()` asks for a restart BEFORE
/// the dying reader has finished leaving. The stop decision must already
/// have released the reader slot, or the restart's claim fails and nobody
/// ever reads stdin again.
///
/// Runs on local flags: no fd-0 reader is spawned and no process-global
/// stdin state is touched, so it cannot disturb the liveness tests.
#[test]
fn a_restart_requested_while_the_reader_is_leaving_is_not_lost() {
    // A reader is running and `pause()` has latched the detach.
    let started = AtomicBool::new(true);
    let detached = AtomicBool::new(true);
    assert!(
        reader_slot_claim_stop(&started, || detached.load(Ordering::Acquire)),
        "a detached reader must decide to stop"
    );
    // `resume()`: clear the latch, then ask for a reader. The old reader
    // has not run another instruction since its stop decision.
    detached.store(false, Ordering::Release);
    assert!(
        reader_slot_claim_start(&started),
        "restart lost: the stopping reader still held the reader slot"
    );
    // The respawned reader owns the slot; a second request is a no-op.
    assert!(!reader_slot_claim_start(&started));
}

/// The other order: `resume()` clears the latch before the reader looks.
/// The reader keeps running and no second reader may be started on fd 0.
#[test]
fn a_resume_that_beats_the_stop_check_keeps_the_one_reader() {
    let started = AtomicBool::new(true);
    let detached = AtomicBool::new(true);
    detached.store(false, Ordering::Release);
    assert!(!reader_slot_claim_start(&started));
    assert!(!reader_slot_claim_stop(&started, || detached.load(Ordering::Acquire)));
    assert!(started.load(Ordering::Acquire));
}

/// Hammer the handshake from two threads: a "reader" that stops whenever
/// it sees the latch and a "main" that pauses/resumes. After every
/// resume the slot must be owned — by the surviving reader or by the
/// restart — never stranded.
#[test]
fn pause_resume_storm_never_strands_the_slot() {
    use std::sync::Arc;
    let started = Arc::new(AtomicBool::new(true));
    let detached = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));
    let reader = {
        let (started, detached, done) = (started.clone(), detached.clone(), done.clone());
        std::thread::spawn(move || {
            while !done.load(Ordering::Acquire) {
                // A live reader polls the latch between reads; one that
                // stopped waits to be "respawned" by main's claim.
                if started.load(Ordering::Acquire) {
                    reader_slot_claim_stop(&started, || detached.load(Ordering::Acquire));
                }
                std::hint::spin_loop();
            }
        })
    };
    for _ in 0..200_000 {
        detached.store(true, Ordering::Release); // pause()
        detached.store(false, Ordering::Release); // resume(): clear …
        reader_slot_claim_start(&started); // … then ensure a reader
        assert!(
            started.load(Ordering::Acquire),
            "resume() returned with no reader owning fd 0"
        );
    }
    done.store(true, Ordering::Release);
    reader.join().unwrap();
}
