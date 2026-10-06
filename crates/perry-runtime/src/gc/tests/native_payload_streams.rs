//! STREAM-PAYLOAD-DESIGN Z8/Z9/Z10: the collector-facing witnesses of
//! native-payload streams, on the test rot13 family
//! (`node_stream::native_hooks::tests`). Their sabotages run from
//! `every_stream_sabotage_makes_its_witness_red` there.
use crate::node_stream::native_hooks::tests::{
    new_rot13, pump, rot13_bytes, FamilyReset, Rot13Opts, CREATED, DATA, DROPPED, RELEASED, STEPS,
    STEP_JOBS_SEEN,
};
use crate::value::{JSValue, TAG_UNDEFINED};
use std::sync::atomic::Ordering;

fn finalized() -> usize {
    crate::native_handle::PAYLOAD_FINALIZED.load(Ordering::SeqCst)
}

fn sv(s: &str) -> f64 {
    let ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

use crate::node_stream::native_hooks::tests::name;

fn handle(stream: f64) -> i64 {
    (stream.to_bits() & crate::value::POINTER_MASK) as i64
}

extern "C" fn sink_data(
    _c: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    chunk: f64,
) -> f64 {
    let mut out = Vec::new();
    crate::node_stream::test_append_chunk_bytes(chunk, &mut out);
    DATA.with(|d| d.borrow_mut().extend_from_slice(&out));
    f64::from_bits(TAG_UNDEFINED)
}

fn on_data(stream: f64, info: *const crate::closure::JsFunctionInfo) {
    let l = crate::value::js_nanbox_pointer(crate::closure::js_closure_alloc(info, 0) as i64);
    crate::node_stream::js_node_stream_method_on(handle(stream), name("data"), l);
}

fn buffer(bytes: &[u8]) -> f64 {
    crate::node_stream::test_buffer_value_from_bytes(bytes)
}

/// Anonymous resident bytes: heap growth, not code-page residency. Total RSS
/// also counts file-backed text pages, which fault in with whatever code
/// first runs and differ between binaries by hundreds of KB with no heap
/// meaning. `smaps_rollup` "Anonymous:" is the heap; where it is missing, Rss
/// minus RssFile from `/proc/self/status`.
fn anon_rss_bytes() -> usize {
    fn kb(text: &str, key: &str) -> Option<usize> {
        text.lines()
            .find_map(|l| l.strip_prefix(key))
            .and_then(|r| r.split_whitespace().next())
            .and_then(|n| n.parse::<usize>().ok())
            .map(|k| k * 1024)
    }
    if let Ok(t) = std::fs::read_to_string("/proc/self/smaps_rollup") {
        if let Some(b) = kb(&t, "Anonymous:") {
            return b;
        }
    }
    let t = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    match (kb(&t, "VmRSS:"), kb(&t, "RssFile:")) {
        (Some(rss), Some(file)) => rss.saturating_sub(file),
        _ => 0,
    }
}

/// Z8: churn. Every stream that completes releases its codec at autoDestroy
/// (no GC needed), every destroyed one at destroy(), and once the objects are
/// unreachable a full collection finalizes every cell: nothing keeps a
/// finished stream alive (no static holds a step job).
#[test]
fn z8_churn_releases_every_codec_at_completion_and_drops_every_payload() {
    let _reset = FamilyReset::new();
    const N: usize = 2000;
    let input = vec![b'q'; 1024];
    let before_cells = finalized();
    let mut rss = Vec::new();
    for batch in 0..8 {
        for _ in 0..N / 8 {
            let scope = crate::gc::RuntimeHandleScope::new();
            let rot = scope.root_nanbox_f64(new_rot13(Rot13Opts::default()));
            on_data(rot.get_nanbox_f64(), crate::fn_info!(sink_data, 1));
            crate::node_stream::js_node_stream_method_write(
                handle(rot.get_nanbox_f64()),
                buffer(&input),
                f64::from_bits(TAG_UNDEFINED),
                f64::from_bits(TAG_UNDEFINED),
            );
            crate::node_stream::js_node_stream_method_end(
                handle(rot.get_nanbox_f64()),
                f64::from_bits(TAG_UNDEFINED),
            );
            pump();
        }
        DATA.with(|d| d.borrow_mut().clear());
        // Measure what survives a full collection, not the garbage a run of
        // minors has not reclaimed yet.
        crate::gc::js_gc_collect();
        rss.push(anon_rss_bytes());
        eprintln!("z8 batch {batch}: anon_rss={}", rss[rss.len() - 1]);
    }
    assert_eq!(CREATED.load(Ordering::SeqCst), N);
    assert_eq!(
        RELEASED.load(Ordering::SeqCst),
        N,
        "autoDestroy released every completed codec, before any collection"
    );
    for _ in 0..N {
        let scope = crate::gc::RuntimeHandleScope::new();
        let rot = scope.root_nanbox_f64(new_rot13(Rot13Opts::default()));
        crate::node_stream::js_node_stream_method_destroy(
            handle(rot.get_nanbox_f64()),
            f64::from_bits(TAG_UNDEFINED),
        );
        pump();
    }
    assert_eq!(RELEASED.load(Ordering::SeqCst), 2 * N, "destroy() released");
    assert_eq!(DROPPED.load(Ordering::SeqCst), 2 * N, "created == dropped");
    crate::gc::js_gc_collect();
    crate::gc::js_gc_collect();
    let cells = finalized() - before_cells;
    eprintln!(
        "z8: created={} released={} dropped={} cells finalized={cells} anon rss warm={} peak={} growth={}",
        CREATED.load(Ordering::SeqCst),
        RELEASED.load(Ordering::SeqCst),
        DROPPED.load(Ordering::SeqCst),
        rss[2],
        rss[2..].iter().max().unwrap(),
        rss[2..].iter().max().unwrap() - rss[2..].iter().min().unwrap()
    );
    assert!(
        cells >= 2 * N - 16,
        "the dead streams' cells were swept: {cells} of {}",
        2 * N
    );
    let warm = *rss[2..].iter().min().unwrap();
    let peak = *rss[2..].iter().max().unwrap();
    assert!(
        peak - warm < 4 << 20,
        "anonymous RSS flat after warmup: {}",
        peak - warm
    );
}

thread_local! {
    static Z9_MOVED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// In every `data`: a minor collection (which moves young strings) and a
/// burst of young garbage that reuses the evacuated space.
extern "C" fn data_then_collect(
    closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    chunk: f64,
) -> f64 {
    let mut out = Vec::new();
    crate::node_stream::test_append_chunk_bytes(chunk, &mut out);
    DATA.with(|d| d.borrow_mut().extend_from_slice(&out));
    let stream = crate::closure::js_closure_get_capture_f64(closure, 0);
    let before = crate::node_stream::native_hooks::test_inflight_chunk(stream);
    crate::gc::gc_collect_minor();
    for i in 0..2000 {
        let s = format!("garbage-garbage-garbage-{i:08}");
        let _ = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
    }
    let after = crate::node_stream::native_hooks::test_inflight_chunk(stream);
    if before.to_bits() != after.to_bits() {
        Z9_MOVED.with(|m| m.set(m.get() + 1));
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// Z9: a collection that moves the chunk in flight, between two steps of one
/// write, leaves the output correct: the runner borrows the input for one
/// step only and re-derives it from the traced chunk.
#[test]
fn z9_moving_gc_inside_data_keeps_the_output_correct() {
    let _reset = FamilyReset::new();
    // Compiled code would emit barriers: young objects move in a minor. Only
    // precise roots, so a stale native pointer is not rescued by a
    // conservative pin.
    struct Barriers;
    impl Drop for Barriers {
        fn drop(&mut self) {
            crate::gc::js_gc_write_barriers_emitted(0);
        }
    }
    crate::gc::js_gc_write_barriers_emitted(1);
    let _barriers = Barriers;
    let _no_stack = super::support::ConservativeScanDisabledGuard::new();
    // The bytes a chunk moved out of read as poison, so a slice held across
    // the move cannot pass by reading the stale copy.
    let _poison =
        crate::arena::ProtectionModeGuard::set(crate::arena::FromSpaceProtection::PoisonOnly);
    let copies_before = crate::gc::copying_minor_cycles();
    Z9_MOVED.with(|m| m.set(0));
    let scope = crate::gc::RuntimeHandleScope::new();
    let rot = scope.root_nanbox_f64(new_rot13(Rot13Opts {
        step_out: 64,
        decode_strings: false,
        ..Default::default()
    }));
    let l = crate::value::js_nanbox_pointer(crate::closure::js_closure_alloc(
        crate::fn_info!(data_then_collect, 1),
        1,
    ) as i64);
    crate::closure::js_closure_set_capture_f64(
        crate::value::js_nanbox_get_pointer(l) as *mut crate::closure::ClosureHeader,
        0,
        rot.get_nanbox_f64(),
    );
    crate::node_stream::js_node_stream_method_on(handle(rot.get_nanbox_f64()), name("data"), l);
    let text: String = (0..64)
        .map(|i| format!("The quick brown fox {i:02} jumps over the lazy dog. "))
        .collect();
    for _ in 0..4 {
        crate::node_stream::js_node_stream_method_write(
            handle(rot.get_nanbox_f64()),
            sv(&text),
            f64::from_bits(TAG_UNDEFINED),
            f64::from_bits(TAG_UNDEFINED),
        );
    }
    pump();
    let moved = Z9_MOVED.with(|m| m.get());
    eprintln!(
        "z9: chunk moved under a step {moved} times; copying minors {}",
        crate::gc::copying_minor_cycles() - copies_before
    );
    assert!(moved > 0, "the witness is armed: a chunk moved mid-write");
    let expected = rot13_bytes(text.repeat(4).as_bytes(), 1);
    let out = DATA.with(|d| d.borrow().clone());
    assert!(out == expected, "output correct after moving collections");
}

/// Z10: a worker that wrote to a deferred stream and exits with the step
/// still queued: its heap tears down, the payload drops exactly once (its
/// cell is finalized), and no step job (no JS) runs after the teardown.
#[test]
fn z10_worker_exit_with_a_step_queued_runs_no_step() {
    let _reset = FamilyReset::new();
    let cells_before = finalized();
    let (jobs, steps) = std::thread::spawn(|| {
        let scope = crate::gc::RuntimeHandleScope::new();
        let rot = scope.root_nanbox_f64(new_rot13(Rot13Opts::default()));
        crate::node_stream::js_node_stream_method_write(
            handle(rot.get_nanbox_f64()),
            buffer(&[b'x'; 1024]),
            f64::from_bits(TAG_UNDEFINED),
            f64::from_bits(TAG_UNDEFINED),
        );
        if crate::node_stream::native_hooks::stream_sabotage("drain_after_teardown") {
            let cell = crate::native_payload::stream_hooks_of(rot.get_nanbox_f64())
                .unwrap()
                .1;
            unsafe { crate::native_handle::finalize_native_handle_at_teardown(cell) };
            crate::timer::js_event_loop_check_phase();
        }
        // Thread exit: the queued immediate dies with the heap.
        (
            STEP_JOBS_SEEN.load(Ordering::SeqCst),
            STEPS.load(Ordering::SeqCst),
        )
    })
    .join()
    .unwrap();
    assert_eq!(jobs, 0, "no step job ran");
    assert_eq!(steps, 0, "no step ran");
    assert_eq!(
        finalized(),
        cells_before + 1,
        "the payload's cell finalized once, at teardown"
    );
}
