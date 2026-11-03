//! Unit tests of the handle registry (a sibling of handle.rs, which holds only the registry itself).
use super::*;
use std::time::Duration;

extern "C" fn rewrite_const_pointer_slot(kind: u32, slot: *mut c_void, ctx: *mut c_void) -> bool {
    assert_eq!(kind, FFI_ROOT_SLOT_RAW_MUT_PTR);
    unsafe {
        *(slot as *mut *const u8) = ctx as *const u8;
    }
    true
}

#[test]
fn const_pointer_root_slot_is_rewritten() {
    let original = 1_u8;
    let replacement = 2_u8;
    let mut slot = &original as *const u8;
    let mut visitor = GcRootVisitor::new(
        rewrite_const_pointer_slot,
        &replacement as *const u8 as *mut c_void,
    );

    assert!(visitor.visit_raw_const_ptr_slot(&mut slot));
    assert_eq!(slot, &replacement as *const u8);
}

#[test]
fn round_trip_simple_value() {
    let _serial = recycle_serial();
    let h = register_handle(42_i64);
    assert_ne!(h, INVALID_HANDLE);
    assert!(h < FFI_HANDLE_ID_END);
    let v = with_handle::<i64, _, _>(h, |v| *v).expect("present");
    assert_eq!(v, 42);
    assert!(drop_handle(h));
    assert!(!handle_exists(h));
}

#[test]
fn mutable_access_persists() {
    let _serial = recycle_serial();
    struct Counter(u32);
    let h = register_handle(Counter(0));
    with_handle_mut::<Counter, _, _>(h, |c| c.0 += 1).expect("present");
    with_handle_mut::<Counter, _, _>(h, |c| c.0 += 1).expect("present");
    let n = with_handle::<Counter, _, _>(h, |c| c.0).expect("present");
    assert_eq!(n, 2);
    drop_handle(h);
}

#[test]
fn iter_handles_of_mut_updates_matching_values() {
    let _serial = recycle_serial();
    struct Counter(u32);
    let a = register_handle(Counter(1));
    let b = register_handle(Counter(10));
    let other = register_handle("not a counter".to_string());

    iter_handles_of_mut::<Counter, _>(|c| c.0 += 1);

    let mut values = Vec::new();
    iter_handles_of::<Counter, _>(|c| values.push(c.0));
    values.sort_unstable();
    assert_eq!(values, vec![2, 11]);

    drop_handle(a);
    drop_handle(b);
    drop_handle(other);
}

#[test]
fn indexed_type_iterates_only_its_live_ids_including_backfill() {
    let _serial = recycle_serial();
    struct Indexed(u32);
    // Registered before the declaration: must be backfilled.
    let early = register_handle(Indexed(1));
    let noise: Vec<Handle> = (0..64).map(|i| register_handle(i as u64)).collect();
    index_handle_type::<Indexed>();
    index_handle_type::<Indexed>(); // idempotent: no duplicate slot/ids
    let late = register_handle(Indexed(2));

    let slot = indexed_slot(TypeId::of::<Indexed>()).expect("declared type is indexed");
    let mut listed = lock_index(slot).clone();
    listed.sort_unstable();
    let mut want = vec![early, late];
    want.sort_unstable();
    assert_eq!(listed, want, "the side list holds exactly the live ids");

    iter_handles_of_mut::<Indexed, _>(|v| v.0 += 10);
    let mut values = Vec::new();
    iter_handles_of::<Indexed, _>(|v| values.push(v.0));
    values.sort_unstable();
    assert_eq!(values, vec![11, 12]);
    let mut ids = Vec::new();
    iter_handle_ids_of::<Indexed, _>(|id| ids.push(id));
    ids.sort_unstable();
    assert_eq!(ids, want);

    // Removal (drop and take) leaves the side list.
    drop_handle(early);
    assert_eq!(take_handle::<Indexed>(late).map(|v| v.0), Some(12));
    assert!(lock_index(slot).is_empty());
    let mut after = 0;
    iter_handles_of::<Indexed, _>(|_| after += 1);
    assert_eq!(after, 0);

    // Other types are neither listed nor affected.
    assert!(indexed_slot(TypeId::of::<u64>()).is_none());
    let mut n = 0;
    iter_handles_of::<u64, _>(|_| n += 1);
    assert!(n >= noise.len());
    for h in noise {
        drop_handle(h);
    }
}

#[test]
fn type_mismatch_returns_none() {
    let _serial = recycle_serial();
    let h = register_handle(42_i64);
    // Same handle, wrong type — no value comes back.
    let r = with_handle::<String, _, _>(h, |s| s.clone());
    assert!(r.is_none());
    drop_handle(h);
}

#[test]
fn handles_are_unique() {
    let _serial = recycle_serial();
    let a = register_handle(1_i32);
    let b = register_handle(2_i32);
    assert_ne!(a, b);
    drop_handle(a);
    drop_handle(b);
}

// ----------------------------------------------------------------
// Id-recycling freelist.
//
// The registry is process-wide and the default test harness runs
// these in parallel, so the reuse-sensitive tests below serialize on
// `RECYCLE_TEST_LOCK` and assert the *recycling contract* (a freed id
// is reused, fresh-id consumption stays bounded) rather than a fixed id
// value, while excluding other registry fixtures from the interval. It
// still fails against a no-reclaim `drop_handle` (the freed id never
// lands on the freelist, so it is never reused and id consumption is
// unbounded). The bounding invariant is tested in isolation against a
// local native registry with a small queue capacity.
// ----------------------------------------------------------------

// Every fixture that drains or observes post-removal ids shares this lock,
// including the sibling registration_tests module and its worker lifetime.
pub(super) static RECYCLE_TEST_LOCK: Mutex<()> = Mutex::new(());

/// Recycling tests run one at a time, each on a thread that owns a heap
/// (with the runtime linked), so the ids it retires are its own to decide.
pub(super) fn recycle_serial() -> std::sync::MutexGuard<'static, ()> {
    let serial = RECYCLE_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    #[cfg(feature = "runtime-link")]
    perry_runtime::gc::gc_init();
    serial
}

/// One tick drain, then two full traces in which no word names any id:
/// the first ages every parked id past its young epoch, the second
/// releases it. Ids are reissued only through that proof.
pub(super) fn collect_unobserved() {
    drain_quarantined_handles();
    for _ in 0..2 {
        crate::handle_lifecycle::trace_phase_for_tests(0);
        crate::handle_lifecycle::trace_phase_for_tests(1);
    }
}

/// One full trace whose heap holds `held` as `HANDLE_TAG` words.
fn trace_observing(held: &[Handle]) {
    crate::handle_lifecycle::trace_phase_for_tests(0);
    for &id in held {
        assert!(crate::handle_lifecycle::observe_for_tests(boxed(id)));
    }
    crate::handle_lifecycle::trace_phase_for_tests(1);
}

pub(super) fn boxed(id: Handle) -> u64 {
    0xFFF9_0000_0000_0000 | id as u64
}

/// Whether `id` could be registered again right now (registers and retires
/// it reusably when it can, leaving the pool as it found it).
pub(super) fn reissuable(id: Handle) -> bool {
    let Ok(identity) =
        REGISTRATIONS.begin_registration_with_id(id, NativeRegistrationKind::Payload)
    else {
        return false;
    };
    assert!(REGISTRATIONS.publish(identity));
    assert!(REGISTRATIONS.begin_retirement_of(identity));
    assert!(REGISTRATIONS.finish_retirement_reusable(identity));
    true
}

#[test]
fn a_dropped_id_is_not_reissued_by_tick_drains() {
    let _serial = recycle_serial();
    let h = register_handle(1_u8);
    assert!(drop_handle(h));
    for _ in 0..5 {
        drain_quarantined_handles();
    }
    assert!(!reissuable(h), "a tick drain must not recycle a retired id");
    collect_unobserved();
    assert!(reissuable(h), "two traces naming nothing release the id");
}

#[test]
fn young_survivors_do_not_raise_the_trace_trigger() {
    use crate::handle_lifecycle::{trace_trigger_for_tests, MIN_TRIGGER};
    let _serial = recycle_serial();
    collect_unobserved();
    let ids: Vec<Handle> = (0..3 * MIN_TRIGGER)
        .map(|i| {
            let h = register_handle(i as u32);
            assert!(drop_handle(h));
            h
        })
        .collect();
    // One trace keeps every id as young: they say nothing about what the
    // program holds, so the trigger must not grow on them (it used to
    // become twice the survivors and doubled every trace).
    trace_observing(&[]);
    assert!(ids.iter().all(|&h| !reissuable(h)));
    assert_eq!(trace_trigger_for_tests(), (MIN_TRIGGER, 0));
    // The next trace decides them: the ids a word names are the live
    // ones, and the trigger is twice those, never below the floor.
    let held = &ids[..MIN_TRIGGER];
    trace_observing(held);
    assert_eq!(trace_trigger_for_tests(), (2 * MIN_TRIGGER, MIN_TRIGGER));
    assert!(ids[MIN_TRIGGER..].iter().all(|&h| reissuable(h)));
    collect_unobserved();
    assert_eq!(trace_trigger_for_tests(), (MIN_TRIGGER, 0));
}

#[test]
fn an_id_named_by_a_traced_word_is_never_reissued() {
    let _serial = recycle_serial();
    let h = register_handle(2_u8);
    assert!(drop_handle(h));
    for _ in 0..4 {
        trace_observing(&[h]);
        assert!(!reissuable(h), "a JS value still holds the id");
    }
    collect_unobserved();
    assert!(reissuable(h));
}

#[test]
fn weak_key_verdict_agrees_with_the_release() {
    let _serial = recycle_serial();
    let h = register_handle(3_u8);
    assert!(drop_handle(h));
    crate::handle_lifecycle::trace_phase_for_tests(0);
    assert!(
        crate::handle_lifecycle::is_live_for_tests(boxed(h)),
        "a young parked id keeps its weak entries"
    );
    crate::handle_lifecycle::trace_phase_for_tests(1);
    assert!(!reissuable(h));
    crate::handle_lifecycle::trace_phase_for_tests(0);
    assert!(!crate::handle_lifecycle::is_live_for_tests(boxed(h)));
    crate::handle_lifecycle::trace_phase_for_tests(1);
    assert!(
        reissuable(h),
        "the trace that deleted its weak entries releases it"
    );
    assert!(crate::handle_lifecycle::is_live_for_tests(boxed(h)));
}

#[test]
fn a_weak_key_named_by_a_traced_word_stays_live() {
    let _serial = recycle_serial();
    let h = register_handle(4_u8);
    assert!(drop_handle(h));
    collect_unobserved();
    let h = register_handle(5_u8);
    assert!(drop_handle(h));
    trace_observing(&[]);
    crate::handle_lifecycle::trace_phase_for_tests(0);
    assert!(!crate::handle_lifecycle::is_live_for_tests(boxed(h)));
    assert!(crate::handle_lifecycle::observe_for_tests(boxed(h)));
    assert!(crate::handle_lifecycle::is_live_for_tests(boxed(h)));
    crate::handle_lifecycle::trace_phase_for_tests(1);
    assert!(!reissuable(h));
    collect_unobserved();
    assert!(reissuable(h));
}

#[test]
fn a_grace_deadline_outlives_unobserved_traces() {
    let _serial = recycle_serial();
    let h = register_handle(6_u8);
    assert!(drop_handle_until(
        h,
        Instant::now() + Duration::from_secs(3600)
    ));
    collect_unobserved();
    crate::handle_lifecycle::trace_phase_for_tests(0);
    assert!(crate::handle_lifecycle::is_live_for_tests(boxed(h)));
    crate::handle_lifecycle::trace_phase_for_tests(1);
    assert!(!reissuable(h));
    let r = reserve_handle_id();
    free_handle_id_until(r, Instant::now() - Duration::from_secs(1));
    collect_unobserved();
    assert!(reissuable(r), "an elapsed deadline adds no hold");
}

/// Register `value`, reporting whether `register_handle` REUSED a parked
/// id rather than minting a fresh one (the recycling contract). A pop
/// leaves the fresh-id counter untouched; a fresh mint advances it, which a
/// no-reclaim build would do on every register. Returns `(handle, reused)`.
fn register_observing_reuse<T: 'static + Send + Sync>(value: T) -> (Handle, bool) {
    let before = REGISTRATIONS.next_fresh_id_for_tests();
    let handle = register_handle(value);
    let reused = REGISTRATIONS.next_fresh_id_for_tests() == before;
    (handle, reused)
}

/// Free `id`, then register `value` and keep retrying until we observe a
/// REUSE (the new registration drew a parked id instead of minting fresh),
/// returning the reused handle. Each non-reusing attempt is dropped so it
/// re-parks an id for the next try.
///
/// A freed id now sits in QUARANTINE until [`drain_quarantined_handles`]
/// promotes it to the freelist (the ABA fix), so each attempt drains first
/// — exactly what the host event loop does once per tick. A no-reclaim
/// `drop_handle` would quarantine NOTHING, so the drain promotes nothing
/// and reuse never happens: the contract still fails hard against a build
/// that doesn't reclaim.
///
/// The bounded retry is what makes the reuse assertion both robust and
/// meaningful on the process-wide freelist. All registry fixtures in this
/// binary now share RECYCLE_TEST_LOCK, so no other fixture may drain or
/// consume a row during this interval. The existing bounded retry still
/// distinguishes reuse from a removal path that never queues any ids.
fn drop_then_register_reusing<T: 'static + Send + Sync>(id: Handle, value: T) -> Handle
where
    T: Clone,
{
    assert!(drop_handle(id), "the id to recycle must have been live");
    for _ in 0..10_000 {
        // Promote prior-tick quarantined ids to the freelist (the host
        // pump's per-tick drain), then observe whether register reuses one.
        collect_unobserved();
        let (handle, reused) = register_observing_reuse(value.clone());
        if reused {
            return handle;
        }
        // This attempt minted fresh; retire it before checking reuse again.
        assert!(drop_handle(handle));
    }
    panic!(
        "register_handle never reused a freed id across 10000 attempts — \
         ids are not being recycled (a no-reclaim drop_handle would do this)"
    );
}

#[test]
fn register_drop_register_reuses_a_freed_id() {
    // End-to-end: a register/drop/register cycle reuses the freed id rather
    // than minting a second fresh one. A no-reclaim `drop_handle` parks
    // nothing, so `register_handle` would always mint fresh and
    // `drop_then_register_reusing` would never observe reuse — a hard fail.
    let _serial = recycle_serial();

    let h1 = register_handle(7_i64);
    let h2 = drop_then_register_reusing(h1, 9_i64);
    assert_eq!(with_handle::<i64, _, _>(h2, |v| *v), Some(9));
    assert!(drop_handle(h2));
}

#[test]
fn reused_id_carries_no_stale_state() {
    let _serial = recycle_serial();

    // Register a String, drop it, then register a different type under the
    // RECYCLED id. The recycled id must resolve to the NEW value with the
    // NEW type — never the prior String (cross-request bleed). The String
    // is no longer reachable because its entry was removed at drop.
    //
    // `drop_then_register_reusing` guarantees the second register actually
    // reused the freed id, so this test exercises the recycle path — it
    // would never silently pass on a no-reclaim registry where the
    // stale-state question is moot.
    let first = register_handle("stale".to_string());
    let second = drop_then_register_reusing(first, 1234_i64);
    assert!(
        with_handle::<String, _, _>(second, |s| s.clone()).is_none(),
        "recycled id must not expose the prior handle's value or type"
    );
    assert_eq!(with_handle::<i64, _, _>(second, |v| *v), Some(1234));
    drop_handle(second);
}

#[test]
fn freed_id_is_not_reusable_until_drained_no_cross_request_bleed() {
    // The ABA / use-after-recycle regression. Models the HTTP cross-request
    // body bleed in handle-registry terms (the layer where the hazard lives,
    // independent of perry-ext-http's reaper plumbing): a response R1
    // is registered under id `h`; the handler returns before `res.end()` and
    // the request is later finalized, freeing `h`; within the SAME tick a new
    // request registers its response R2; then a stale `res.write`/`res.end`
    // from the retired handler fires, carrying only the bare id `h`, and
    // mutates whatever `h` resolves to.
    //
    // The hazard: if the free made `h` immediately reusable, the new
    // registration would re-occupy `h` with R2 and the stale write would
    // mutate R2 — one request's body bleeding into another's. The quarantine
    // makes a freed id reusable only after `drain_quarantined_handles` (the
    // host pump's per-tick promotion, which runs only AFTER the finalizing
    // handler's microtasks — and thus any stale writes — have drained). So
    // within the tick `h` resolves to NOTHING, and the stale write no-ops
    // against the empty slot exactly as it did before the freelist existed.
    let _serial = recycle_serial();

    // A `ServerResponse`-shaped stand-in: the field a bleed would corrupt.
    #[derive(Clone)]
    struct Response {
        buffered_body: Vec<u8>,
    }

    // Register R1, capturing its id (the stale `res` holds this).
    let h = register_handle(Response {
        buffered_body: b"request-1-body".to_vec(),
    });

    // Finalize R1 — it is removed from the registry and its id freed
    // (quarantined, NOT yet on the freelist).
    assert!(drop_handle(h));

    // A new request registers R2 in the SAME tick (no drain yet). Because
    // `h` is quarantined, register_handle CANNOT hand `h` back — R2 gets a
    // different id. This is the load-bearing assertion: revert the
    // quarantine (recycle straight to the freelist) and `h2 == h`, so the
    // stale write below lands on R2 and the final assertion fails.
    let h2 = register_handle(Response {
        buffered_body: b"request-2-body".to_vec(),
    });
    assert_ne!(
        h2, h,
        "a just-freed id must not be reusable within the same tick — \
         reusing it lets a stale handle holder corrupt the new request"
    );

    // The stale write fires against `h`. With the quarantine, `h` resolves
    // to nothing — a safe no-op (mirrors how the HTTP FFI's
    // `get_handle::<ServerResponse>(h)` returns None → early-return).
    let appended = with_handle_mut::<Response, _, _>(h, |r| {
        r.buffered_body.extend_from_slice(b"-STALE-WRITE");
    });
    assert!(
        appended.is_none(),
        "a stale write to a finalized (quarantined) id must hit an empty \
         slot, not a live object"
    );

    // R2's body is pristine — the stale write did NOT bleed into it.
    let r2_body =
        with_handle::<Response, _, _>(h2, |r| r.buffered_body.clone()).expect("R2 is still live");
    assert_eq!(
        r2_body, b"request-2-body",
        "the second request's response body must be untouched by the \
         stale write to the finalized first request's id"
    );

    // After a tick boundary (drain), `h` is safely reusable again — and a
    // fresh registration under it carries no stale R1 state.
    collect_unobserved();
    let h3 = drop_then_register_reusing(
        h2,
        Response {
            buffered_body: b"request-3-body".to_vec(),
        },
    );
    let r3_body =
        with_handle::<Response, _, _>(h3, |r| r.buffered_body.clone()).expect("R3 is live");
    assert_eq!(r3_body, b"request-3-body");
    drop_handle(h3);
}

#[test]
fn deadline_gated_id_survives_ticks_until_grace_then_recycles() {
    // The LONG-ASYNC use-after-recycle regression — the residual window the
    // one-tick quarantine does NOT cover. Models the HTTP reaper finalizing a
    // response that never ended (`writable_ended` unset — peer disconnect or
    // server force-close) while its handler is still suspended on a slow
    // upstream `await`: response R1 is registered under id `h`; the reaper
    // finalizes the request WITHOUT the handler ending the response
    // (`drop_handle_until(h, grace_deadline)`) because the peer is gone, so
    // `h` enters the deadline-gated quarantine; many pump ticks pass (each
    // draining); then the handler resumes — still within the grace window —
    // and its stale `res.write` fires carrying only the bare id `h`.
    //
    // With a one-tick quarantine, the first drain would have promoted `h`
    // (and a new request could have re-minted it), so the resumed write would
    // mutate a LIVE response — cross-request body corruption that is NOT a
    // write-after-end (the handler never called `end()`). The deadline-gated
    // quarantine keeps `h` parked across EVERY tick until the grace deadline
    // passes, so the stale write no-ops against an empty slot for the whole
    // window — then `h` recycles cleanly once the request is definitively
    // dead.
    let _serial = recycle_serial();

    #[derive(Clone)]
    struct Response {
        buffered_body: Vec<u8>,
    }

    // Register R1 (the suspended handler holds this bare id).
    let h = register_handle(Response {
        buffered_body: b"request-1-body".to_vec(),
    });

    // The reaper finalizes WITHOUT end — defer recycling until a grace
    // deadline well in the future (stand-in for `requestTimeout`).
    let grace = Instant::now() + Duration::from_secs(3600);
    assert!(drop_handle_until(h, grace));

    // Several pump ticks pass. Each drain must leave `h` parked because its
    // deadline has not elapsed — so a new request can never be handed `h`.
    // (Fail-before: a one-tick quarantine promotes `h` on the first drain,
    // making the assertion below fail.)
    for _ in 0..5 {
        collect_unobserved();
        let probe = register_handle(0_i64);
        assert_ne!(
            probe, h,
            "a deadline-gated id must not be recycled before its grace \
             deadline — reusing it lets a long-suspended handler corrupt a \
             new request's response"
        );
        drop_handle(probe);
    }

    // The stale write fires against `h`. While `h` is parked it maps to
    // nothing, so the write no-ops against an empty slot — no bleed.
    let appended = with_handle_mut::<Response, _, _>(h, |r| {
        r.buffered_body.extend_from_slice(b"-STALE-WRITE");
    });
    assert!(
        appended.is_none(),
        "a stale write to a deadline-gated id must hit an empty slot, not a \
         live object"
    );

    // Drain the far-future entry away so it can't leak into later tests
    // (its deadline hasn't elapsed, so this is a no-op for `h` — but it
    // clears the one-tick tier). Then prove the elapsed-deadline path DOES
    // recycle: a separate id parked with an already-past deadline is
    // promoted by the very next drain and reused cleanly, carrying no stale
    // state. This is the post-grace tick, modeled deterministically (no
    // sleep) with an id we fully control.
    collect_unobserved();
    let dead = register_handle(Response {
        buffered_body: b"to-be-finalized".to_vec(),
    });
    assert!(drop_handle_until(
        dead,
        Instant::now() - Duration::from_secs(1)
    ));
    // The elapsed-deadline entry is eligible immediately; retry to absorb
    // parallel tests racing the shared freelist (same pattern as
    // `drop_then_register_reusing`).
    let mut recycled = None;
    for _ in 0..10_000 {
        collect_unobserved();
        let candidate = register_handle(Response {
            buffered_body: b"request-2-body".to_vec(),
        });
        if candidate == dead {
            recycled = Some(candidate);
            break;
        }
        drop_handle(candidate);
    }
    let h2 = recycled.expect("an elapsed-deadline id must recycle once its grace passes");
    let body = with_handle::<Response, _, _>(h2, |r| r.buffered_body.clone())
        .expect("the recycled id resolves to the new response");
    assert_eq!(
        body, b"request-2-body",
        "the recycled id carries the NEW response, never the finalized one"
    );
    drop_handle(h2);

    // `h`'s far-future deadline entry stays parked — that is the point of
    // the test. The freelist is bounded and the process-wide registry
    // tolerates a held id, so this leaks nothing that matters across tests.
}

#[test]
fn live_handles_never_share_an_id() {
    // Serialized: this test drains the quarantine, which mutates the shared
    // recycle state the other recycle-sensitive tests depend on.
    let _serial = recycle_serial();

    // Recycling must never hand the same id to two live handles. Hold a
    // batch live (none dropped) and assert every id is distinct, then
    // free them and re-allocate the same count, again all-distinct.
    fn batch_all_distinct() -> Vec<Handle> {
        let live: Vec<Handle> = (0..256).map(|i| register_handle(i as i64)).collect();
        let mut sorted = live.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            live.len(),
            "no two concurrently-live handles may share an id"
        );
        live
    }

    let first = batch_all_distinct();
    for h in &first {
        drop_handle(*h);
    }
    // Promote the just-quarantined ids so the next batch reuses them (the
    // host pump's per-tick drain); they must still all be mutually distinct.
    collect_unobserved();
    let second = batch_all_distinct();
    for h in &second {
        drop_handle(*h);
    }
}

#[test]
fn ordinary_quarantine_is_bounded() {
    let registry = NativeRegistrationRegistry::new(1, 20, 4);
    let ids: Vec<_> = (0..12)
        .map(|_| {
            let identity = registry
                .begin_registration(NativeRegistrationKind::Reserved)
                .unwrap();
            assert!(registry.publish(identity));
            identity
        })
        .collect();
    for identity in ids {
        assert!(registry.begin_retirement_of(identity));
        assert!(registry.finish_retirement(identity, crate::NativeQuarantine::NextDrain));
    }
    assert_eq!(
        registry.drain(Instant::now()),
        4,
        "ordinary quarantine retains at most four of twelve retirements"
    );
}

#[test]
fn churn_does_not_exhaust_the_id_band() {
    let _serial = recycle_serial();

    // Register/drop churn past the id band size while never holding more
    // than one handle live. With recycling the fresh counter barely
    // moves — each drop refills the freelist the next register drains —
    // so cumulative allocations are decoupled from fresh-id consumption.
    // Without recycling this loop would advance the counter by
    // `iterations` and `next_fresh_handle_id` would PANIC at the
    // `FFI_HANDLE_ID_END` exhaustion check (a fail-before of a different
    // shape: the no-reclaim build can't even complete the loop).
    //
    // Measure the fresh-counter delta directly. Concurrent tests mint a
    // bounded handful of fresh ids; recycling keeps OUR contribution near
    // zero, so the total delta stays tiny in absolute terms.
    let iterations = FFI_HANDLE_ID_END as usize + 8192;
    let before = REGISTRATIONS.next_fresh_id_for_tests();
    for n in 0..iterations {
        let h = register_handle(n as i64);
        assert!(drop_handle(h));
        // The dropped id is quarantined, not yet reusable — drain it back
        // to the freelist (the host pump's per-tick promotion) so the next
        // register reuses it rather than minting fresh and exhausting the
        // band. A no-reclaim build quarantines nothing, so this drain is a
        // no-op and the counter still runs away.
        collect_unobserved();
    }
    let after = REGISTRATIONS.next_fresh_id_for_tests();
    let fresh_minted = (after - before) as usize;
    assert!(
        fresh_minted < 4096,
        "fresh-id consumption ({fresh_minted}) over {iterations} \
         register/drop cycles should stay tiny once ids recycle; a \
         no-reclaim registry would mint one per allocation and exhaust \
         the band"
    );
}

// ----------------------------------------------------------------
// Reserved-id recycling (#6441).
//
// `reserve_handle_id` mints a globally-unique id WITHOUT storing a
// value in `HANDLES` — for a subsystem (perry-ext-net's socket map)
// that keeps its own object map. Before `free_handle_id` existed
// nothing ever returned a reserved id, so a long-running server
// leaked the whole `[1, 0x40000)` band and then crashed. These tests
// pin the two halves of the fix: reserved ids now recycle through the
// same quarantine as `drop_handle` ids, and exhaustion degrades to
// `INVALID_HANDLE` instead of a panic.
// ----------------------------------------------------------------

#[test]
fn fresh_id_or_exhausted_flags_the_band_boundary() {
    let registry = NativeRegistrationRegistry::new(FFI_HANDLE_ID_END - 1, FFI_HANDLE_ID_END, 4);
    assert_eq!(
        registry
            .begin_registration(NativeRegistrationKind::Reserved)
            .unwrap()
            .numeric_id(),
        FFI_HANDLE_ID_END - 1
    );
    assert_eq!(
        registry.begin_registration(NativeRegistrationKind::Reserved),
        Err(crate::NativeRegistrationError::IdExhausted)
    );
}

#[test]
fn reserve_free_reserve_recycles_the_id() {
    let _serial = recycle_serial();

    // Reserve/free churn far past the band size while holding at most one
    // id live. With `free_handle_id` recycling each id through the
    // quarantine, the fresh counter barely moves — cumulative reservations
    // decouple from fresh-id consumption, exactly as `register`/`drop` do.
    //
    // Before this fix `reserve_handle_id` had no `free_*` twin, so this
    // loop would advance the counter by `iterations`, exhaust the band, and
    // (post-#6441) start returning `INVALID_HANDLE` — tripping the
    // `assert_ne!` below. (Pre-#6441 it panicked outright.) Either way a
    // no-recycle build cannot complete the loop.
    let iterations = FFI_HANDLE_ID_END as usize + 8192;
    let before = REGISTRATIONS.next_fresh_id_for_tests();
    for _ in 0..iterations {
        let id = reserve_handle_id();
        assert_ne!(
            id, INVALID_HANDLE,
            "reserve_handle_id must not run out of ids once freed ids recycle"
        );
        free_handle_id(id);
        // Promote the just-quarantined id back to the freelist (the host
        // pump's per-tick drain) so the next reserve reuses it.
        collect_unobserved();
    }
    let fresh_minted = (REGISTRATIONS.next_fresh_id_for_tests() - before) as usize;
    assert!(
        fresh_minted < 4096,
        "fresh-id consumption ({fresh_minted}) over {iterations} \
         reserve/free cycles should stay tiny once reserved ids recycle"
    );
}

#[test]
fn freed_reserved_id_not_reusable_until_drained() {
    // The ABA guarantee for reserved ids, mirroring
    // `freed_id_is_not_reusable_until_drained_no_cross_request_bleed`: a
    // socket id freed on `'close'` must not be handed to a *new* reservation
    // within the same tick, or a stale JS `socket` reference dispatched
    // before the next drain would alias the new socket (the exact aliasing
    // class #6407 fixes). While quarantined the id maps to nothing, so the
    // stale dispatch spends against an empty slot.
    let _serial = recycle_serial();

    let h = reserve_handle_id();
    assert_ne!(h, INVALID_HANDLE);
    free_handle_id(h);

    // No drain yet: `h` sits in the quarantine, NOT the freelist, so neither
    // a fresh reservation nor a registration can re-mint it this tick. Only
    // the serialized recycle tests call `drain`, and this test holds the
    // lock, so `h` provably stays quarantined here.
    let other_reserved = reserve_handle_id();
    assert_ne!(
        other_reserved, h,
        "a just-freed reserved id must not be reusable within the same tick"
    );
    let other_registered = register_handle(0_i64);
    assert_ne!(
        other_registered, h,
        "a just-freed reserved id must not leak into register_handle this tick"
    );

    // After a drain boundary the id is promoted and eventually reused.
    free_handle_id(other_reserved);
    let reused = drop_then_register_reusing(other_registered, 55_i64);
    assert_eq!(with_handle::<i64, _, _>(reused, |v| *v), Some(55));
    drop_handle(reused);
    collect_unobserved();
}

#[test]
fn free_handle_id_until_holds_reserved_id_past_ticks() {
    // The deadline-gated twin for reserved ids (the reaper's peer-disconnect
    // path in handle-registry terms): a reserved id freed with a future
    // grace deadline stays parked across EVERY drain until the deadline
    // elapses, then recycles cleanly.
    let _serial = recycle_serial();

    let h = reserve_handle_id();
    assert_ne!(h, INVALID_HANDLE);
    free_handle_id_until(h, Instant::now() + Duration::from_secs(3600));

    for _ in 0..5 {
        collect_unobserved();
        let probe = reserve_handle_id();
        assert_ne!(
            probe, h,
            "a deadline-gated reserved id must not recycle before its grace \
             deadline elapses"
        );
        free_handle_id(probe);
    }

    // An id parked with an already-elapsed deadline is promoted by the very
    // next drain and reused (retry to absorb parallel tests racing the
    // shared freelist, same pattern as `drop_then_register_reusing`).
    let dead = reserve_handle_id();
    free_handle_id_until(dead, Instant::now() - Duration::from_secs(1));
    let mut recycled = false;
    for _ in 0..10_000 {
        collect_unobserved();
        let candidate = reserve_handle_id();
        if candidate == dead {
            recycled = true;
            free_handle_id(candidate);
            break;
        }
        free_handle_id(candidate);
    }
    assert!(
        recycled,
        "an elapsed-deadline reserved id must recycle once its grace passes"
    );
    // `h`'s far-future entry stays parked — the point of the test. The
    // freelist is bounded, so a single held id leaks nothing that matters.
    collect_unobserved();
}

#[test]
fn free_handle_id_ignores_invalid_handle() {
    let _serial = recycle_serial();
    // `reserve_handle_id` returns `INVALID_HANDLE` on exhaustion, so callers
    // free its result unconditionally; freeing the sentinel must be a no-op
    // (never park `0` for reuse — it is the "no handle" value).
    free_handle_id(INVALID_HANDLE);
    free_handle_id_until(INVALID_HANDLE, Instant::now());
    collect_unobserved();
    // `register_handle` never returns `INVALID_HANDLE`, so if `0` had been
    // parked and handed back this would fail its own non-null invariant.
    let h = register_handle(1_i64);
    assert_ne!(h, INVALID_HANDLE);
    drop_handle(h);
}
