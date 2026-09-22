//! P10 post-ABI tests. Each asserts its subject ran — a job was invoked, a
//! context was reclaimed, a class was rejected — not merely that nothing threw.
//!
//! The end-to-end hop (foreign thread posts → the owner's turn runs it) lives
//! in `event_pump::agent_loop_tests`, next to the mechanism it builds on,
//! because only that module has the route harness the primary agent needs.

use super::*;
use std::sync::atomic::AtomicUsize;

/// A context whose destruction is observable, so "who owns this now" is a
/// question the tests can answer rather than assume.
struct Ctx {
    value: u64,
    dropped: &'static AtomicUsize,
}

impl Drop for Ctx {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::SeqCst);
    }
}

/// The router in `dispatch_staged` is a sequence of range tests with P1 as the
/// fall-through, so a class this module shares with another subsystem would not
/// fail loudly — it would hand a boxed `HostJob` to the net sink, which reads
/// the token's low bits as a socket id. Assert the bands are disjoint.
#[test]
fn the_posted_job_class_collides_with_no_other_token_space() {
    for class in CLASS_MIN..=CLASS_MAX {
        let token = Token((class << ID_BITS) | 1);
        assert!(owns(token), "class {class:#x} must be this module's");
        assert!(
            !crate::turnloop_proc::owns(token),
            "class {class:#x} must not also be P2's"
        );
        assert!(
            !crate::turnloop_pool::owns(token),
            "class {class:#x} must not also be P4's"
        );
        assert_ne!(
            token,
            crate::event_pump::TIMER_TOKEN,
            "class {class:#x} must not be P3's timer"
        );
    }
    // P1's net classes are 1..=7 and it is the router's fall-through, so the
    // guard that keeps a posted job away from the net sink is `owns` answering
    // false for them — not a range test on P1's side, which has none.
    for class in 1u64..=7 {
        assert!(
            !owns(Token((class << ID_BITS) | 1)),
            "P1's class {class} must not read as a posted job"
        );
    }
}

/// Two jobs must be distinguishable in a trace even when two threads post at
/// the same moment, which a per-thread counter could not promise.
#[test]
fn every_job_token_is_distinct_and_in_class() {
    let a = job_token();
    let b = job_token();
    assert_ne!(a, b, "two jobs must not share a token");
    for t in [a, b] {
        assert_eq!(t.0 >> ID_BITS, CLASS_JOB, "token carries the job class");
        assert_ne!(t.0 & ID_MASK, 0, "a zero id reads as unset in a trace");
        assert!(owns(t), "a minted token must route back here");
    }
}

/// The ABI's whole contract is the sign of the return code. If a variant's
/// `consumed()` and its `code()` sign ever disagree, a C caller either leaks
/// every context or frees one the runtime is about to invoke.
#[test]
fn the_return_code_sign_is_exactly_the_ownership_rule() {
    for (outcome, consumed) in [
        (Posted::Accepted, true),
        (Posted::QueuedUnwoken, true),
        (Posted::NoRoute, false),
        (Posted::Again, false),
    ] {
        assert_eq!(
            outcome.consumed(),
            consumed,
            "{outcome:?} must {} the caller's context",
            if consumed { "take" } else { "leave" }
        );
        assert_eq!(
            outcome.code() >= 0,
            consumed,
            "{outcome:?}: the sign of the code IS the ownership rule"
        );
    }
    // The four codes must stay distinct, or a caller cannot tell "no loop
    // exists, use your fallback" from "retry this".
    let codes = [
        Posted::Accepted.code(),
        Posted::QueuedUnwoken.code(),
        Posted::NoRoute.code(),
        Posted::Again.code(),
    ];
    for (i, a) in codes.iter().enumerate() {
        for b in &codes[i + 1..] {
            assert_ne!(a, b, "two outcomes share a code");
        }
    }
}

extern "C" fn must_not_run(_ctx: *mut c_void) {
    panic!("a refused post must never invoke the callback");
}

/// A thread acting for an agent nobody speaks for is the `NoRoute` case: the
/// binding's own fallback is the right answer, and it can only take it if the
/// context is still its own. Asserts the reclaim really happens — the `Drop`
/// runs exactly once, after the caller takes the box back, and never before.
#[test]
fn a_post_with_no_route_leaves_the_context_with_the_caller() {
    per_test_global! {
        /// Single-threaded: this test has no route / a null callback, so both
        /// the increment and the assertion happen on this thread. No adoption.
        static DROPPED: AtomicUsize = AtomicUsize::new(0);
    }

    // A worker agent id nobody has claimed a route for. Taken on a thread of
    // its own so the process's primary route — which other tests own — is
    // untouched by this one.
    std::thread::spawn(|| {
        crate::agent::enter_agent_for_test(u64::MAX - 7);
        assert!(
            !available(),
            "an agent with no route must not advertise a post path"
        );

        let ctx = Box::into_raw(Box::new(Ctx {
            value: 0x00C0_FFEE,
            dropped: &DROPPED,
        }));
        // SAFETY: `ctx` is a live leaked box; the callback would be the only
        // other consumer and this post is expected to be refused.
        let outcome = unsafe { post(must_not_run, ctx.cast()) };

        assert_eq!(outcome, Posted::NoRoute, "no route exists for this agent");
        assert!(!outcome.consumed(), "a refusal must not take the context");
        assert_eq!(
            DROPPED.load(Ordering::SeqCst),
            0,
            "the runtime must not have dropped a context it refused"
        );

        // The caller's fallback path: take the context back. This is the whole
        // point of the negative-code half of the rule.
        // SAFETY: the post was refused, so this box is still ours and unaliased.
        drop(unsafe { Box::from_raw(ctx) });
        assert_eq!(
            DROPPED.load(Ordering::SeqCst),
            1,
            "the caller reclaimed and dropped its context exactly once"
        );
    })
    .join()
    .expect("unrouted-agent test thread");
}

/// The ABI must refuse a null callback rather than call through it, and must
/// report it on the *negative* side so the caller keeps its context.
#[test]
fn a_null_callback_is_refused_without_taking_the_context() {
    per_test_global! {
        /// Single-threaded: this test has no route / a null callback, so both
        /// the increment and the assertion happen on this thread. No adoption.
        static DROPPED: AtomicUsize = AtomicUsize::new(0);
    }
    let ctx = Box::into_raw(Box::new(Ctx {
        value: 1,
        dropped: &DROPPED,
    }));
    // SAFETY: a live leaked box; a null callback is refused before any use.
    let code = unsafe { super::abi::js_perry_agent_post(None, ctx.cast()) };
    assert!(code < 0, "a null callback must report a non-consuming code");
    assert_eq!(DROPPED.load(Ordering::SeqCst), 0, "nothing was taken");
    // SAFETY: refused, so the box is still ours.
    let reclaimed = unsafe { Box::from_raw(ctx) };
    assert_eq!(reclaimed.value, 1, "the context was not touched");
    drop(reclaimed);
    assert_eq!(DROPPED.load(Ordering::SeqCst), 1);
}

/// `dispatched()` is what a binding's test asserts to prove the owner carried
/// its work. A counter that starts at anything but zero on a fresh thread, or
/// that leaks another thread's count, would make that assertion vacuous.
#[test]
fn the_liveness_counter_is_per_thread_and_starts_at_zero() {
    let seen = std::thread::spawn(|| {
        assert_eq!(dispatched(), 0, "a fresh thread has run no posted job");
        DISPATCHED.with(|n| n.set(41));
        dispatched()
    })
    .join()
    .expect("counter test thread");
    assert_eq!(seen, 41, "the counter reads back this thread's own value");
    assert_eq!(
        dispatched(),
        0,
        "another thread's count must not leak into this one"
    );
}
