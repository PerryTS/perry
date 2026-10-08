//! `PayloadBuffer` (#11919): a payload's raw working memory is counted as the
//! payload's external bytes and given back by close before any collection,
//! with the sweep's drop as the backstop.

use super::super::*;
use super::support::*;
use crate::native_payload::{self, NativePayloadFamily, PayloadBuffer, PayloadBufferOwner};
use crate::native_payload_buffer::live_bytes;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicUsize, Ordering};

static DROPS: AtomicUsize = AtomicUsize::new(0);

/// A codec-shaped payload: an owner and two working buffers.
struct Codec {
    owner: PayloadBufferOwner,
    window: (NonNull<u8>, usize),
    scratch: (NonNull<u8>, usize),
}

impl Codec {
    fn new(window: usize, scratch: usize) -> Self {
        let owner = PayloadBufferOwner::new();
        let window = PayloadBuffer::alloc(&owner, window).unwrap();
        let scratch = PayloadBuffer::alloc(&owner, scratch).unwrap();
        Self {
            owner,
            window,
            scratch,
        }
    }
    fn external_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.owner.bytes()
    }
}

impl Drop for Codec {
    fn drop(&mut self) {
        unsafe {
            PayloadBuffer::release(&self.owner, self.window.0, self.window.1);
            PayloadBuffer::release(&self.owner, self.scratch.0, self.scratch.1);
        }
        DROPS.fetch_add(1, Ordering::SeqCst);
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

struct PrototypeReset;
impl Drop for PrototypeReset {
    fn drop(&mut self) {
        native_payload::reset_payload_prototypes_for_tests();
    }
}

fn full_collection() {
    let _ =
        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
}

const WINDOW: usize = 4 << 20;
const SCRATCH: usize = 64 << 10;

/// Grow mid-stream, restate, then close: the buffer bytes and the cell's
/// external bytes both return at close, before any collection runs.
#[test]
fn payload_buffer_close_releases_bytes_before_any_collection() {
    let _guard = GcTestIsolationGuard::with_realm_bootstrapped();
    let _reset = PrototypeReset;
    gc_register_mutable_root_scanner(native_payload::scan_payload_prototype_roots_mut);
    DROPS.store(0, Ordering::SeqCst);
    let buffers_before = live_bytes();
    let external_before = policy::external_side_live_bytes();
    let codec = Codec::new(WINDOW, SCRATCH);
    let bytes = codec.external_bytes();
    let value = native_payload::alloc(&FAMILY, codec, bytes, &[]);
    assert_eq!(live_bytes(), buffers_before + WINDOW + SCRATCH);
    assert_eq!(policy::external_side_live_bytes(), external_before + bytes);
    // Mid-stream growth of the scratch, restated through set_external_bytes.
    let grown = unsafe {
        let codec = native_payload::payload_mut::<Codec>(value, &FAMILY).unwrap();
        let (ptr, len) = codec.scratch;
        codec.scratch = PayloadBuffer::grow(&codec.owner, ptr, len, 4 * SCRATCH).unwrap();
        codec.external_bytes()
    };
    native_payload::set_external_bytes(value, &FAMILY, grown);
    assert_eq!(live_bytes(), buffers_before + WINDOW + 4 * SCRATCH);
    assert_eq!(policy::external_side_live_bytes(), external_before + grown);
    let collections = gc_total_collection_count();
    assert_eq!(
        native_payload::close(value, &FAMILY),
        native_payload::CloseOutcome::Closed
    );
    assert_eq!(
        gc_total_collection_count(),
        collections,
        "premise: no collection ran"
    );
    assert_eq!(DROPS.load(Ordering::SeqCst), 1, "close dropped the payload");
    assert_eq!(live_bytes(), buffers_before, "close freed the buffers");
    assert_eq!(
        policy::external_side_live_bytes(),
        external_before,
        "close released the external bytes"
    );
    let _no_conservative = ConservativeScanDisabledGuard::new();
    full_collection();
    assert_eq!(
        DROPS.load(Ordering::SeqCst),
        1,
        "the sweep drops nothing again"
    );
    assert_eq!(live_bytes(), buffers_before);
}

/// Nobody closes: the sweep that finds the object dead frees the buffers.
#[test]
fn payload_buffer_dead_payload_is_freed_by_the_sweep() {
    let _guard = GcTestIsolationGuard::with_realm_bootstrapped();
    let _reset = PrototypeReset;
    gc_register_mutable_root_scanner(native_payload::scan_payload_prototype_roots_mut);
    DROPS.store(0, Ordering::SeqCst);
    let buffers_before = live_bytes();
    let external_before = policy::external_side_live_bytes();
    for _ in 0..16 {
        let codec = Codec::new(SCRATCH, SCRATCH);
        let bytes = codec.external_bytes();
        let _ = native_payload::alloc(&FAMILY, codec, bytes, &[]);
    }
    assert_eq!(live_bytes(), buffers_before + 16 * 2 * SCRATCH);
    let _no_conservative = ConservativeScanDisabledGuard::new();
    full_collection();
    assert_eq!(DROPS.load(Ordering::SeqCst), 16);
    assert_eq!(live_bytes(), buffers_before);
    assert_eq!(policy::external_side_live_bytes(), external_before);
}

/// Churn: 16k open/close cycles, each holding 320 KiB of buffers. The bytes
/// return every time, and RSS grows no more than the same object churn with
/// empty buffers does (the control isolates the collector's own heap).
#[test]
fn payload_buffer_churn_keeps_bytes_and_rss_flat() {
    let _guard = GcTestIsolationGuard::with_realm_bootstrapped();
    let _reset = PrototypeReset;
    gc_register_mutable_root_scanner(native_payload::scan_payload_prototype_roots_mut);
    let buffers_before = live_bytes();
    let drift = |window: usize, scratch: usize| {
        let mut samples = Vec::new();
        for round in 0..10 {
            for _ in 0..1600 {
                let codec = Codec::new(window, scratch);
                let bytes = codec.external_bytes();
                let value = native_payload::alloc(&FAMILY, codec, bytes, &[]);
                native_payload::close(value, &FAMILY);
                assert_eq!(live_bytes(), buffers_before);
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
