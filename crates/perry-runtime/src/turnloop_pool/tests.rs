//! P4 acceptance: real work on the real pool, delivered on the real loop.
//!
//! Nothing here is mocked. Every job runs on a turnloop blocking-pool thread
//! and its result comes back through `Loop::turn`, so a fixture that never
//! reached the pool cannot pass: each test asserts *which thread ran the work*
//! (by comparing thread ids), and every test that claims a delivery also
//! asserts the counters moved (DESIGN §11, CLAUDE.md "a gate must assert its
//! subject was live").

use std::cell::RefCell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

use super::*;

/// A one-shot gate every blocked job waits on, opened once from the test.
///
/// Deliberately not a `Barrier`: a barrier of two pairs whichever two parties
/// arrive first, so several pool threads would release *each other* and the
/// test would deadlock against the ones that were left. What these tests need
/// is "hold every pool thread until I say so", which is a latch.
#[derive(Default)]
struct Gate {
    open: Mutex<bool>,
    changed: Condvar,
}

impl Gate {
    fn wait(&self) {
        let mut open = self
            .open
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while !*open {
            open = self
                .changed
                .wait(open)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    fn open(&self) {
        *self
            .open
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        self.changed.notify_all();
    }
}

/// What a delivery recorded, owned, so assertions read like the program.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Rec {
    Done(&'static str, Vec<u8>),
    Cancelled(&'static str),
    Failed(&'static str, ErrorKind),
}

thread_local! {
    static EVENTS: RefCell<Vec<Rec>> = const { RefCell::new(Vec::new()) };
}

fn record(rec: Rec) {
    EVENTS.with(|events| events.borrow_mut().push(rec));
}

fn events() -> Vec<Rec> {
    EVENTS.with(|events| events.borrow().clone())
}

/// Read a NaN-boxed JS string back out of the heap, so a rooting test can
/// assert the *contents* survived rather than that the bits are unchanged —
/// an evacuation rewrites the bits, which is the whole point.
fn read_js_string(bits: u64) -> String {
    let value = crate::JSValue::from_bits(bits);
    assert!(value.is_any_string(), "the parked value is still a string");
    let ptr = crate::value::js_get_string_pointer_unified(f64::from_bits(bits))
        as *const crate::StringHeader;
    assert!(
        crate::value::addr_class::is_plausible_heap_addr(ptr as usize),
        "live string pointer"
    );
    // The runtime API, not open-coded `ptr + size_of::<StringHeader>()`: the
    // payload offset is the runtime's to know, and hand-rolling it is what
    // `scripts/string_payload_access_inventory.py` ratchets against.
    let owned = unsafe { crate::OwnedStringBytes::copy_from_header(ptr) };
    String::from_utf8_lossy(owned.as_bytes()).into_owned()
}

struct Fixture {
    submitted: u64,
    completed: u64,
    cancelled: u64,
    failed: u64,
}

impl Fixture {
    fn start() -> Self {
        assert!(
            crate::event_pump::install_net_loop_for_test(),
            "the host must provide a turnloop loop, or every assertion below is vacuous"
        );
        EVENTS.with(|events| events.borrow_mut().clear());
        super::reset_for_test();
        Fixture {
            submitted: submitted_total(),
            completed: completed_total(),
            cancelled: cancelled_total(),
            failed: failed_total(),
        }
    }

    /// Deltas since the fixture started. The counters are process-wide and
    /// these tests share a process, so only a delta is meaningful.
    fn delta(&self) -> (u64, u64, u64, u64) {
        (
            submitted_total() - self.submitted,
            completed_total() - self.completed,
            cancelled_total() - self.cancelled,
            failed_total() - self.failed,
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        super::shutdown_current_thread();
        crate::event_pump::reset_net_loop_for_test();
        EVENTS.with(|events| events.borrow_mut().clear());
    }
}

/// Turn the loop until `done` or the deadline. Bounded: a job that never
/// completes must fail the test rather than hang the suite.
fn pump_until(done: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        crate::event_pump::pump_net_for_test(Duration::from_millis(5));
    }
    done()
}

fn pump_until_events(n: usize) -> bool {
    pump_until(|| events().len() >= n)
}

// ── The core contract ───────────────────────────────────────────────────────

#[test]
fn a_job_runs_on_a_pool_thread_and_delivers_on_the_submitting_thread() {
    let fixture = Fixture::start();
    let owner = std::thread::current().id();
    let job = submit(
        move || (std::thread::current().id(), b"payload".to_vec()),
        move |delivery| {
            let Delivery::Done((worker, bytes)) = delivery else {
                panic!("a job that ran must deliver Done, got {delivery:?}");
            };
            // The whole point of the phase: the CPU work did not run on the
            // thread that owns the JS heap.
            assert_ne!(
                worker, owner,
                "the work must run on a pool thread, not on the submitting thread"
            );
            assert_eq!(
                std::thread::current().id(),
                owner,
                "the delivery must run on the submitting thread, where JS lives"
            );
            record(Rec::Done("job", bytes));
        },
    )
    .expect("the pool accepts a job on a loop-owning thread");
    assert_eq!(outstanding(), 1, "an accepted job is outstanding");
    assert!(has_pending_jobs(), "an accepted job keeps the loop alive");
    assert!(
        pump_until_events(1),
        "the job must complete: {:?}",
        events()
    );
    assert_eq!(events(), vec![Rec::Done("job", b"payload".to_vec())]);
    assert_eq!(outstanding(), 0, "a delivered job leaves the table");
    assert_eq!(fixture.delta(), (1, 1, 0, 0));
    // A cancel after delivery names nothing; it must not cancel whatever took
    // the slot, which is why ids are monotonic and never reused.
    assert!(!cancel(job), "a delivered job is unknown to cancel");
}

/// FNV-1a over the whole buffer: a real byte-by-byte CPU pass, and a value
/// that differs if a single byte is lost or reordered crossing the pool.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[test]
fn real_cpu_work_over_a_large_payload_is_byte_exact_in_both_directions() {
    // Not a sleep and not a counter: four megabytes hashed byte by byte on a
    // pool thread, with a transformed slice carried back. The payload is large
    // enough that the job cannot have finished before `submit` returned, and
    // the result is wrong if a single byte is lost in either direction.
    let fixture = Fixture::start();
    let source: Vec<u8> = (0..4 * 1024 * 1024u32).map(|i| (i % 251) as u8).collect();
    let expected_hash = fnv1a(&source);
    let expected_tail: Vec<u8> = source[source.len() - 4096..]
        .iter()
        .map(|b| b ^ 0x5a)
        .collect();
    let payload_len = source.len();
    submit(
        move || {
            let hash = fnv1a(&source);
            let tail: Vec<u8> = source[source.len() - 4096..]
                .iter()
                .map(|b| b ^ 0x5a)
                .collect();
            (source.len(), hash, tail)
        },
        move |delivery| {
            let Delivery::Done((len, hash, tail)) = delivery else {
                panic!("expected Done, got {delivery:?}");
            };
            assert_eq!(len, payload_len, "the whole payload reached the pool");
            assert_eq!(hash, expected_hash, "the hash must be byte-identical");
            assert_eq!(tail, expected_tail, "the bytes must come back intact");
            record(Rec::Done("cpu", tail));
        },
    )
    .expect("accepted");
    assert!(pump_until_events(1), "the CPU job must complete");
    assert_eq!(events().len(), 1);
    assert_eq!(fixture.delta(), (1, 1, 0, 0));
}

#[test]
fn many_jobs_all_complete_and_the_pool_runs_them_in_parallel() {
    let fixture = Fixture::start();
    const JOBS: usize = 32;
    let ran = Arc::new(AtomicUsize::new(0));
    for i in 0..JOBS {
        let ran = ran.clone();
        submit(
            move || {
                ran.fetch_add(1, Ordering::AcqRel);
                // Long enough that the queue backs up behind this worker, so
                // the other pool threads must take the rest. Without it a
                // single fast thread could serve all 32 and the parallelism
                // assertion below would pass or fail by timing.
                std::thread::sleep(Duration::from_millis(2));
                (i, std::thread::current().id())
            },
            move |delivery| {
                let Delivery::Done((got, worker)) = delivery else {
                    panic!("expected Done, got {delivery:?}");
                };
                assert_eq!(got, i, "each job delivers its own result");
                EVENTS.with(|events| {
                    events.borrow_mut().push(Rec::Done("n", vec![got as u8]));
                });
                WORKERS.with(|w| w.borrow_mut().push(worker));
            },
        )
        .expect("accepted");
    }
    assert!(
        pump_until_events(JOBS),
        "all {JOBS} jobs must complete, got {}",
        events().len()
    );
    assert_eq!(ran.load(Ordering::Acquire), JOBS, "every job's work ran");
    assert_eq!(fixture.delta(), (JOBS as u64, JOBS as u64, 0, 0));
    // The pool is bounded and shared (DESIGN D8, default 4 threads). More than
    // one worker must have served these jobs, or the "pool" is a single
    // background thread wearing a pool's name.
    let distinct = WORKERS.with(|w| {
        let mut ids: Vec<ThreadId> = w.borrow().clone();
        ids.sort_by_key(|id| format!("{id:?}"));
        ids.dedup();
        ids.len()
    });
    assert!(
        distinct > 1,
        "a shared pool must serve {JOBS} jobs from more than one thread, saw {distinct}"
    );
    WORKERS.with(|w| w.borrow_mut().clear());
}

thread_local! {
    static WORKERS: RefCell<Vec<ThreadId>> = const { RefCell::new(Vec::new()) };
}

// ── Cancellation ────────────────────────────────────────────────────────────

#[test]
fn a_cancelled_job_still_delivers_exactly_once() {
    let fixture = Fixture::start();
    // Fill every pool thread so the job under test is still queued when the
    // cancel lands — cancellation is best-effort by design (DESIGN D8), and
    // this is what makes the race deterministic instead of hopeful.
    let gate = Arc::new(Gate::default());
    let blockers = 8;
    for _ in 0..blockers {
        let gate = gate.clone();
        submit(
            move || gate.wait(),
            |_| record(Rec::Done("blocker", vec![])),
        )
        .expect("accepted");
    }
    let job = submit(
        || panic!("a job cancelled while queued must never run"),
        |delivery| match delivery {
            Delivery::Cancelled => record(Rec::Cancelled("victim")),
            other => panic!("expected Cancelled, got {other:?}"),
        },
    )
    .expect("accepted");
    assert!(cancel(job), "a queued job is cancellable");
    assert!(
        !cancel(job),
        "a second cancel of the same job must find it already cancelled or gone"
    );
    // Release every blocker at once.
    gate.open();
    assert!(
        pump_until(|| events().contains(&Rec::Cancelled("victim"))),
        "the cancelled job must still produce exactly one delivery: {:?}",
        events()
    );
    let victims = events()
        .iter()
        .filter(|e| **e == Rec::Cancelled("victim"))
        .count();
    assert_eq!(victims, 1, "exactly once, not zero and not twice");
    assert!(pump_until(|| {
        events()
            .iter()
            .filter(|e| **e == Rec::Done("blocker", vec![]))
            .count()
            == blockers
    }));
    let (submitted, _, cancelled, _) = fixture.delta();
    assert_eq!(submitted, blockers as u64 + 1);
    assert_eq!(cancelled, 1, "the counter names the cancelled job");
    assert_eq!(outstanding(), 0);
}

#[test]
fn a_panicking_job_is_reported_as_failed_and_does_not_poison_the_pool() {
    let fixture = Fixture::start();
    submit(
        || panic!("deliberate panic inside a pool job"),
        |delivery| match delivery {
            Delivery::Failed(e) => record(Rec::Failed("panic", e.kind)),
            other => panic!("expected Failed, got {other:?}"),
        },
    )
    .expect("accepted");
    assert!(pump_until_events(1), "the panic must be delivered");
    assert_eq!(events(), vec![Rec::Failed("panic", ErrorKind::Other)]);
    // The pool must still work afterwards — a panicking job kills its own
    // delivery, not the worker.
    submit(
        || 7u8,
        |d| {
            let Delivery::Done(v) = d else {
                panic!("expected Done")
            };
            record(Rec::Done("after", vec![v]));
        },
    )
    .expect("accepted");
    assert!(pump_until_events(2), "the pool survives a panicking job");
    assert_eq!(events()[1], Rec::Done("after", vec![7]));
    let (submitted, completed, _, failed) = fixture.delta();
    assert_eq!((submitted, completed, failed), (2, 1, 1));
}

// ── Backpressure ────────────────────────────────────────────────────────────

#[test]
fn a_full_pool_queue_refuses_rather_than_growing_without_bound() {
    let fixture = Fixture::start();
    // The queue is bounded (DESIGN D8); the loop's own operation table is too.
    // Submit until something refuses, then assert the refusal is backpressure
    // and that nothing was silently accepted-and-dropped.
    let gate = Arc::new(Gate::default());
    let mut accepted = 0usize;
    let mut refusal = None;
    for _ in 0..20_000 {
        let gate = gate.clone();
        match submit(move || gate.wait(), |_| record(Rec::Done("fill", vec![]))) {
            Ok(_) => accepted += 1,
            Err(e) => {
                refusal = Some(e);
                break;
            }
        }
    }
    let refusal = refusal.expect("a bounded queue must refuse eventually");
    assert_eq!(
        refusal,
        SubmitError::Busy,
        "a full queue is backpressure, not a hard failure"
    );
    assert!(accepted > 0, "the pool accepted work before it refused");
    assert_eq!(
        outstanding(),
        accepted,
        "a refused submission must not appear in the table"
    );
    let (submitted, _, _, _) = fixture.delta();
    assert_eq!(
        submitted, accepted as u64,
        "the refused submissions are not counted as jobs"
    );
    assert!(
        refused_total() > 0,
        "the refusal counter is what says the fallback ran instead of the pool"
    );

    // With the pool provably saturated, the fallback is exercised for real
    // rather than simulated: `submit_or_run_inline` must run the work on THIS
    // thread and deliver before it returns, so a caller settles exactly once
    // whichever path ran. A binding that relied on the pool being infinite
    // would otherwise lose the job silently under load.
    let owner = std::thread::current().id();
    let ran_inline = !submit_or_run_inline(
        move || std::thread::current().id(),
        move |delivery| {
            let Delivery::Done(worker) = delivery else {
                panic!("the inline fallback must deliver Done, got {delivery:?}");
            };
            assert_eq!(worker, owner, "the fallback runs on the calling thread");
            record(Rec::Done("inline", vec![]));
        },
    );
    assert!(ran_inline, "a saturated pool must refuse and fall back");
    assert!(
        events().contains(&Rec::Done("inline", vec![])),
        "the fallback delivered before returning"
    );

    gate.open();
    assert!(
        pump_until(|| outstanding() == 0),
        "every accepted job still completes, {} left",
        outstanding()
    );
}

// ── Occupancy classes (PerryTS/turnloop#42) ─────────────────────────────────

/// `submit_long` rides turnloop's `Occupancy::Long` set, which a saturated
/// bounded set cannot starve: with EVERY bounded worker provably held on a
/// gate (read from `turnloop::pool_stats`, not assumed from a thread count),
/// a long job still runs on a pool thread and delivers on the submitting one.
/// Had it gone to the bounded set it would queue behind the gate and the
/// first assertion would time out.
#[test]
fn a_long_job_runs_while_every_bounded_worker_is_held() {
    let fixture = Fixture::start();
    let gate = Arc::new(Gate::default());
    let mut held = 0u64;
    for _ in 0..64 {
        let gate = gate.clone();
        submit(move || gate.wait(), |_| record(Rec::Done("held", vec![])))
            .expect("the bounded queue has room for the holding jobs");
        held += 1;
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let stats = turnloop::pool_stats();
        if stats.threads > 0 && stats.busy == stats.threads {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "every bounded worker must be held before the long job, or this proves nothing: {stats:?}"
        );
        std::thread::sleep(Duration::from_millis(1));
    }

    let owner = std::thread::current().id();
    submit_long(
        move || std::thread::current().id(),
        move |delivery| {
            let Delivery::Done(worker) = delivery else {
                panic!("a long job that ran must deliver Done, got {delivery:?}");
            };
            assert_ne!(worker, owner, "the long job runs on a pool thread");
            assert_eq!(
                std::thread::current().id(),
                owner,
                "and delivers on the submitting thread"
            );
            record(Rec::Done("long", vec![]));
        },
    )
    .expect("the long set accepts a job on a loop-owning thread");
    assert!(
        pump_until(|| events().contains(&Rec::Done("long", vec![]))),
        "the long job must complete while the bounded set is held: {:?}",
        events()
    );
    assert!(
        !events().contains(&Rec::Done("held", vec![])),
        "no bounded job may have finished yet — the gate is still shut"
    );

    gate.open();
    assert!(
        pump_until(|| outstanding() == 0),
        "every held job still completes, {} left",
        outstanding()
    );
    let (submitted, completed, cancelled, failed) = fixture.delta();
    assert_eq!(
        (submitted, completed, cancelled, failed),
        (held + 1, held + 1, 0, 0)
    );
}

// ── Rooting ─────────────────────────────────────────────────────────────────

#[test]
fn a_parked_js_value_survives_a_collection_and_reaches_the_delivery() {
    let fixture = Fixture::start();
    // A real heap value, parked across a real collection, exactly as
    // `zlib.gzip(buf, cb)` parks its callback. Without the registered scanner
    // the copying minor would leave the entry pointing at a retired from-space
    // object and the delivery would hand JS a dangling value.
    let text = "pool-rooted-value";
    let value = crate::JSValue::string_ptr(crate::js_string_from_bytes(
        text.as_ptr(),
        text.len() as u32,
    ))
    .bits();
    submit_rooted(
        vec![value],
        || 1u8,
        move |delivery, roots| {
            assert!(matches!(delivery, Delivery::Done(1)));
            assert_eq!(roots.len(), 1, "the parked value comes back");
            record(Rec::Done("rooted", read_js_string(roots[0]).into_bytes()));
        },
    )
    .expect("accepted");
    // Collect while the job is outstanding. The value is reachable from this
    // module's table and from nowhere else.
    crate::gc::js_gc_collect();
    assert!(pump_until_events(1), "the rooted job must complete");
    assert_eq!(
        events(),
        vec![Rec::Done("rooted", text.as_bytes().to_vec())],
        "the parked value must still read as its own string after a collection"
    );
    assert_eq!(fixture.delta(), (1, 1, 0, 0));
}

// ── Lifecycle ───────────────────────────────────────────────────────────────

#[test]
fn shutdown_settles_every_outstanding_job_exactly_once() {
    let fixture = Fixture::start();
    let gate = Arc::new(Gate::default());
    let held = gate.clone();
    submit(move || held.wait(), |_| record(Rec::Done("held", vec![]))).expect("accepted");
    submit(
        || 0u8,
        |delivery| match delivery {
            Delivery::Cancelled => record(Rec::Cancelled("at-shutdown")),
            Delivery::Done(_) => record(Rec::Done("raced", vec![])),
            other => panic!("unexpected {other:?}"),
        },
    )
    .expect("accepted");
    assert!(outstanding() >= 1);
    super::shutdown_current_thread();
    assert_eq!(
        outstanding(),
        0,
        "shutdown leaves no job that could never be delivered"
    );
    let delivered = events().len();
    assert_eq!(
        delivered,
        2,
        "every accepted job got exactly one delivery: {:?}",
        events()
    );
    gate.open();
    let (submitted, completed, cancelled, _) = fixture.delta();
    assert_eq!(submitted, 2);
    assert_eq!(completed + cancelled, 2, "exactly one outcome per job");
    assert!(!has_pending_jobs(), "the keep-alive gate is released");
}

// ── Routing ─────────────────────────────────────────────────────────────────

#[test]
fn the_pool_token_space_is_disjoint_from_every_other_phase() {
    // The router in `agent_loop::dispatch_staged` is a range test and nothing
    // else, so this is the contract, not an inference from a passing workload.
    for id in [1u64, 2, 1_000, ID_MASK] {
        let t = token(OP_JOB, id);
        assert!(owns(t), "P4 owns its own token");
        assert!(!crate::turnloop_proc::owns(t), "P2 must not claim it");
        // P3's single timer token is `Token(u64::MAX)`, i.e. class 255.
        assert_ne!(t, Token(u64::MAX), "P3's timer token");
        assert_eq!(token_parts(t), (OP_JOB, id));
    }
    // P1's classes are 1..=7 and P2's 0x10..=0x1F; neither can be mistaken for
    // a P4 class.
    for class in 1..=0x1Fu64 {
        assert!(!owns(Token((class << ID_BITS) | 1)));
    }
}

#[test]
fn a_stale_completion_for_a_delivered_job_is_dropped() {
    let _fixture = Fixture::start();
    // The shape a shutdown leaves behind: the job is gone from the table but
    // the driver still had its completion staged.
    dispatch(Completion {
        token: token(OP_JOB, 99_999),
        op: None,
        handle: None,
        terminal: true,
        result: OpResult::Blocking(Payload::Boxed(Box::new(1u8))),
    });
    assert_eq!(events(), Vec::new(), "a stale token delivers nothing");
    assert_eq!(outstanding(), 0);
}
