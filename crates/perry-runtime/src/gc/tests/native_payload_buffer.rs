//! `PayloadBuffer` (#11919): a payload's raw working memory is counted as the
//! payload's external bytes and given back by close before any collection,
//! with the sweep's drop as the backstop.
//!
//! Every assertion reads this payload's own counters (its owner's bytes, its
//! drop count, its cell's external bytes), never a process-wide total, so the
//! tests hold under parallel test threads.

use super::super::*;
use super::support::*;
use crate::native_payload::{
    self, CallEnd, CloseOutcome, NativePayloadFamily, PayloadBuffer, PayloadBufferOwner,
};
use std::cell::Cell;
use std::ptr::NonNull;
use std::rc::Rc;

/// What the test keeps of a payload after handing it over: its owner (the
/// buffer count) and its drop count.
#[derive(Default)]
struct Ledger {
    owner: PayloadBufferOwner,
    drops: Cell<usize>,
}

/// A codec-shaped payload: a shared ledger and two working buffers.
struct Codec {
    ledger: Rc<Ledger>,
    window: (NonNull<u8>, usize),
    scratch: (NonNull<u8>, usize),
}

impl Codec {
    fn new(window: usize, scratch: usize) -> (Self, Rc<Ledger>) {
        let ledger = Rc::new(Ledger::default());
        let window = PayloadBuffer::alloc(&ledger.owner, window).unwrap();
        let scratch = PayloadBuffer::alloc(&ledger.owner, scratch).unwrap();
        let codec = Self {
            ledger: ledger.clone(),
            window,
            scratch,
        };
        (codec, ledger)
    }
    fn external_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.ledger.owner.bytes()
    }
}

impl Drop for Codec {
    fn drop(&mut self) {
        let owner = &self.ledger.owner;
        unsafe {
            PayloadBuffer::release(owner, self.window.0, self.window.1);
            PayloadBuffer::release(owner, self.scratch.0, self.scratch.1);
        }
        self.ledger.drops.set(self.ledger.drops.get() + 1);
    }
}

fn install(_proto: &mut native_payload::PayloadPrototype) {}

static FAMILY: NativePayloadFamily = NativePayloadFamily {
    class_id: crate::native_class_ids::CRYPTO_HASH,
    name: "BufferProbe",
    constructor_export: None,
    constructor_length: 0,
    links_owner: false,
    install_prototype: install,
};

/// The same payload in a family whose native calls can hold it busy.
static LINKED: NativePayloadFamily = NativePayloadFamily {
    class_id: crate::native_class_ids::CRYPTO_HASH,
    name: "LinkedBufferProbe",
    constructor_export: None,
    constructor_length: 0,
    links_owner: true,
    install_prototype: install,
};

struct PrototypeReset;
impl PrototypeReset {
    fn new() -> Self {
        native_payload::reset_payload_prototypes_for_tests();
        gc_register_mutable_root_scanner(native_payload::scan_payload_prototype_roots_mut);
        register_runtime_handle_root_scanner_for_tests();
        gc_register_named_mutable_root_scanner(
            "pinned",
            crate::gc::pin::scan_pinned_object_roots_mut,
        );
        Self
    }
}
impl Drop for PrototypeReset {
    fn drop(&mut self) {
        native_payload::reset_payload_prototypes_for_tests();
    }
}

fn full_collection() {
    let _ =
        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
}

/// The bytes this object's own cell states.
fn cell_bytes(value: f64) -> u64 {
    let obj = crate::native_payload::any_object(value).unwrap();
    unsafe {
        let cell = ((*(*obj).meta).native_state & POINTER_MASK)
            as *const crate::native_handle::NativeHandleHeader;
        (*cell).external_bytes
    }
}

const WINDOW: usize = 4 << 20;
const SCRATCH: usize = 64 << 10;

/// Grow mid-stream, restate, then close: the buffer bytes and the cell's
/// external bytes both return at close, before any collection runs.
#[test]
fn payload_buffer_close_releases_bytes_before_any_collection() {
    let _guard = GcTestIsolationGuard::with_realm_bootstrapped();
    let _reset = PrototypeReset::new();
    let (codec, ledger) = Codec::new(WINDOW, SCRATCH);
    let bytes = codec.external_bytes();
    let value = native_payload::alloc(&FAMILY, codec, bytes, &[]);
    assert_eq!(ledger.owner.bytes(), WINDOW + SCRATCH);
    assert_eq!(cell_bytes(value), bytes as u64);
    // Mid-stream growth of the scratch, restated through set_external_bytes.
    let grown = unsafe {
        let codec = native_payload::payload_mut::<Codec>(value, &FAMILY).unwrap();
        let (ptr, len) = codec.scratch;
        codec.scratch = PayloadBuffer::grow(&ledger.owner, ptr, len, 4 * SCRATCH).unwrap();
        codec.external_bytes()
    };
    native_payload::set_external_bytes(value, &FAMILY, grown);
    assert_eq!(ledger.owner.bytes(), WINDOW + 4 * SCRATCH);
    assert_eq!(cell_bytes(value), grown as u64);
    let collections = gc_total_collection_count();
    assert_eq!(native_payload::close(value, &FAMILY), CloseOutcome::Closed);
    assert_eq!(
        gc_total_collection_count(),
        collections,
        "premise: no collection ran"
    );
    assert_eq!(ledger.drops.get(), 1, "close dropped the payload");
    assert_eq!(ledger.owner.bytes(), 0, "close freed the buffers");
    assert_eq!(cell_bytes(value), 0, "close released the external bytes");
    let _no_conservative = ConservativeScanDisabledGuard::new();
    full_collection();
    assert_eq!(ledger.drops.get(), 1, "the sweep drops nothing again");
    assert_eq!(ledger.owner.bytes(), 0);
}

/// A second close finds nothing: no second drop, no second subtraction.
#[test]
fn payload_buffer_double_close_drops_and_subtracts_once() {
    let _guard = GcTestIsolationGuard::with_realm_bootstrapped();
    let _reset = PrototypeReset::new();
    let external_before = policy::external_side_live_bytes();
    let (codec, ledger) = Codec::new(SCRATCH, SCRATCH);
    let bytes = codec.external_bytes();
    let value = native_payload::alloc(&FAMILY, codec, bytes, &[]);
    assert_eq!(native_payload::close(value, &FAMILY), CloseOutcome::Closed);
    let external_after = policy::external_side_live_bytes();
    assert_eq!(external_after, external_before);
    assert_eq!(
        native_payload::close(value, &FAMILY),
        CloseOutcome::AlreadyClosed
    );
    assert_eq!(ledger.drops.get(), 1, "no second drop");
    assert_eq!(ledger.owner.bytes(), 0);
    assert_eq!(cell_bytes(value), 0);
    assert_eq!(
        policy::external_side_live_bytes(),
        external_after,
        "no second subtraction"
    );
}

/// Close while a native call holds the payload busy is deferred: the
/// buffers stay counted and live until the outermost call finishes, then
/// they return at once, before any collection.
#[test]
fn payload_buffer_close_while_busy_releases_when_the_call_finishes() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _reset = PrototypeReset::new();
    let (codec, ledger) = Codec::new(WINDOW, SCRATCH);
    let bytes = codec.external_bytes();
    let value = native_payload::alloc(&LINKED, codec, bytes, &[]);
    let collections = gc_total_collection_count();
    let call = native_payload::enter(value, &LINKED).unwrap();
    assert_eq!(
        native_payload::close(value, &LINKED),
        CloseOutcome::Deferred
    );
    assert_eq!(ledger.drops.get(), 0, "busy: not dropped yet");
    assert_eq!(ledger.owner.bytes(), WINDOW + SCRATCH, "busy: buffers held");
    assert_eq!(cell_bytes(value), bytes as u64, "busy: still counted");
    assert_eq!(call.finish(), Err(CallEnd::Closed));
    assert_eq!(ledger.drops.get(), 1);
    assert_eq!(ledger.owner.bytes(), 0, "buffers freed at finish");
    assert_eq!(cell_bytes(value), 0, "external bytes released at finish");
    assert_eq!(
        gc_total_collection_count(),
        collections,
        "no collection ran"
    );
}

/// Nobody closes: the sweep that finds the object dead frees the buffers.
#[test]
fn payload_buffer_dead_payload_is_freed_by_the_sweep() {
    let _guard = GcTestIsolationGuard::with_realm_bootstrapped();
    let _reset = PrototypeReset::new();
    let mut ledgers = Vec::new();
    for _ in 0..16 {
        let (codec, ledger) = Codec::new(SCRATCH, SCRATCH);
        let bytes = codec.external_bytes();
        let _ = native_payload::alloc(&FAMILY, codec, bytes, &[]);
        assert_eq!(ledger.owner.bytes(), 2 * SCRATCH);
        ledgers.push(ledger);
    }
    let _no_conservative = ConservativeScanDisabledGuard::new();
    full_collection();
    for ledger in &ledgers {
        assert_eq!(ledger.drops.get(), 1, "the sweep dropped it");
        assert_eq!(ledger.owner.bytes(), 0, "and freed its buffers");
    }
}

/// Churn: 16k open/close cycles, each holding 320 KiB of buffers. The bytes
/// return every time, and RSS grows no more than the same object churn with
/// empty buffers does (the control isolates the collector's own heap).
#[test]
fn payload_buffer_churn_keeps_bytes_and_rss_flat() {
    let _guard = GcTestIsolationGuard::with_realm_bootstrapped();
    let _reset = PrototypeReset::new();
    let drift = |window: usize, scratch: usize| {
        let mut samples = Vec::new();
        for round in 0..10 {
            for _ in 0..1600 {
                let (codec, ledger) = Codec::new(window, scratch);
                let bytes = codec.external_bytes();
                let value = native_payload::alloc(&FAMILY, codec, bytes, &[]);
                native_payload::close(value, &FAMILY);
                assert_eq!((ledger.drops.get(), ledger.owner.bytes()), (1, 0));
            }
            let _no_conservative = ConservativeScanDisabledGuard::new();
            full_collection();
            if round >= 2 {
                samples.push(rss_bytes());
            }
        }
        let low = *samples.iter().min().unwrap();
        (*samples.iter().max().unwrap() - low, samples)
    };
    let (control, control_samples) = drift(0, 0);
    let (buffered, buffered_samples) = drift(256 << 10, SCRATCH);
    assert!(
        buffered <= control + (4 << 20),
        "RSS drifted: buffered {buffered_samples:?}, control {control_samples:?}"
    );
}

fn rss_bytes() -> usize {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/self/statm")
            .ok()
            .and_then(|s| s.split_whitespace().nth(1)?.parse::<usize>().ok())
            .unwrap_or(0)
            * 4096
    }
    #[cfg(not(target_os = "linux"))]
    {
        0
    }
}

/// The sabotage leaves the payload to the sweep ("free only on drop"); the
/// close witness must go red.
#[test]
fn payload_buffer_release_on_drop_only_sabotage_turns_the_witness_red() {
    if std::env::var("PERRY_TEST_PAYLOAD_BUFFER_SABOTAGE").is_ok() {
        return;
    }
    let witness =
        "gc::tests::native_payload_buffer::payload_buffer_close_releases_bytes_before_any_collection";
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", witness, "--nocapture", "--test-threads=1"])
        .env("PERRY_TEST_PAYLOAD_BUFFER_SABOTAGE", "release_on_drop_only")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        stdout.contains("running 1 test"),
        "missing witness: {stdout}"
    );
    assert!(
        !result.status.success(),
        "release_on_drop_only must turn {witness} red"
    );
}
