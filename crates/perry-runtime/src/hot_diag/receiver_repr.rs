//! Receiver-representation migration ledger.
//!
//! The classifiers in this module are deliberately diagnostics-only. Every
//! caller first tests [`receiver_repr_on`], so an unarmed process pays one
//! relaxed load and enters none of the range, registry, or ownership probes.

use super::{sink_from_env, write_sink, Sink};
use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const FAMILY_COUNT: usize = 13;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
const SNAPSHOT_EVERY: u64 = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ReceiverReprFamily {
    Common,
    Fetch,
    Zlib,
    Proxy,
    Timer,
    Text,
    Tui,
    AsyncHook,
    AsyncResource,
    SymbolGlobal,
    ExternalBuffer,
    Sab,
    NullStub,
}

impl ReceiverReprFamily {
    #[cfg(test)]
    const ALL: [Self; FAMILY_COUNT] = [
        Self::Common,
        Self::Fetch,
        Self::Zlib,
        Self::Proxy,
        Self::Timer,
        Self::Text,
        Self::Tui,
        Self::AsyncHook,
        Self::AsyncResource,
        Self::SymbolGlobal,
        Self::ExternalBuffer,
        Self::Sab,
        Self::NullStub,
    ];

    const NAMES: [&'static str; FAMILY_COUNT] = [
        "common",
        "fetch",
        "zlib",
        "proxy",
        "timer",
        "text",
        "tui",
        "async_hook",
        "async_resource",
        "symbol_global",
        "external_buffer",
        "sab",
        "null_stub",
    ];

    #[inline]
    const fn index(self) -> usize {
        self as usize
    }
}

struct FamilyCounters {
    constructed: [AtomicU64; FAMILY_COUNT],
    observed_old: [AtomicU64; FAMILY_COUNT],
    observed_wrapped: [AtomicU64; FAMILY_COUNT],
}

impl FamilyCounters {
    const fn new() -> Self {
        Self {
            constructed: [const { AtomicU64::new(0) }; FAMILY_COUNT],
            observed_old: [const { AtomicU64::new(0) }; FAMILY_COUNT],
            observed_wrapped: [const { AtomicU64::new(0) }; FAMILY_COUNT],
        }
    }
}

static COUNTERS: FamilyCounters = FamilyCounters::new();
static BARE_MANAGED: AtomicU64 = AtomicU64::new(0);
static INVALID_POINTER_ZERO: AtomicU64 = AtomicU64::new(0);
static DIRECT_MISMATCH: AtomicU64 = AtomicU64::new(0);
static EVENTS: AtomicU64 = AtomicU64::new(0);
static RECEIVER_REPR_SINK: OnceLock<Option<Sink>> = OnceLock::new();
static RECEIVER_REPR_ON: AtomicBool = AtomicBool::new(false);
static LAST_DUMP: Mutex<Option<Instant>> = Mutex::new(None);

#[cfg(test)]
static TEST_FORCE_ON: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
static TEST_CLASSIFICATION_ENTRIES: AtomicU64 = AtomicU64::new(0);

fn receiver_repr_sink() -> &'static Option<Sink> {
    RECEIVER_REPR_SINK.get_or_init(|| {
        let sink = sink_from_env("PERRY_RECEIVER_REPR_DIAG");
        RECEIVER_REPR_ON.store(sink.is_some(), Ordering::Relaxed);
        sink
    })
}

/// One relaxed load after the environment has been parsed once.
#[inline]
pub fn receiver_repr_on() -> bool {
    if RECEIVER_REPR_SINK.get().is_none() {
        receiver_repr_sink();
    }
    let armed = RECEIVER_REPR_ON.load(Ordering::Relaxed);
    #[cfg(test)]
    let armed = armed || TEST_FORCE_ON.load(Ordering::Relaxed);
    armed
}

#[inline]
pub fn receiver_repr_note_constructed(family: ReceiverReprFamily) {
    COUNTERS.constructed[family.index()].fetch_add(1, Ordering::Relaxed);
    maybe_dump();
}

/// Reserved for the later wrapper PRs; PR 1 records the zero baseline.
#[inline]
pub fn receiver_repr_note_wrapped(family: ReceiverReprFamily) {
    COUNTERS.observed_wrapped[family.index()].fetch_add(1, Ordering::Relaxed);
    maybe_dump();
}

/// Observe a dynamic receiver before a NaN-box tag has been stripped.
///
/// This function intentionally does not repeat the armed test. Its callers are
/// the audited funnels, each with exactly one [`receiver_repr_on`] guard.
#[inline]
pub fn receiver_repr_note_value(value: f64) {
    test_note_classification_entry();
    let bits = value.to_bits();
    if bits & TAG_MASK == POINTER_TAG {
        let addr = (bits & POINTER_MASK) as usize;
        if addr == 0 {
            INVALID_POINTER_ZERO.fetch_add(1, Ordering::Relaxed);
        } else {
            observe_pointer(addr);
        }
    } else if bits >> 48 == 0 && bits != 0 {
        // This is the compatibility shape PR 10 removes: an allocator-owned
        // address bitcast directly to f64 with no pointer tag.
        if unsafe { crate::value::addr_class::try_read_tracked_gc_header(bits as usize) }.is_some()
        {
            BARE_MANAGED.fetch_add(1, Ordering::Relaxed);
        }
    }
    maybe_dump();
}

/// Observe a funnel which has already stripped `POINTER_TAG` by contract.
#[inline]
pub fn receiver_repr_note_decoded_pointer(addr: usize) {
    test_note_classification_entry();
    if addr == 0 {
        INVALID_POINTER_ZERO.fetch_add(1, Ordering::Relaxed);
    } else {
        observe_pointer(addr);
    }
    maybe_dump();
}

#[inline]
fn mark_old(family: ReceiverReprFamily) {
    COUNTERS.observed_old[family.index()].fetch_add(1, Ordering::Relaxed);
}

fn observe_pointer(addr: usize) {
    // The small bands are disjoint, except that timer/text/TUI registries use
    // small ids too. Count every matching semantic family: the old encoding
    // contains no provenance bit with which to choose one of colliding id 1s.
    if crate::value::addr_class::is_common_handle_band(addr) {
        mark_old(ReceiverReprFamily::Common);
    }
    if crate::value::addr_class::is_fetch_handle_band(addr) {
        mark_old(ReceiverReprFamily::Fetch);
    }
    if crate::value::addr_class::is_zlib_handle_band(addr) {
        mark_old(ReceiverReprFamily::Zlib);
    }
    if crate::value::addr_class::is_proxy_id_band(addr) {
        let boxed = f64::from_bits(POINTER_TAG | addr as u64);
        if crate::proxy::js_proxy_is_proxy(boxed) != 0 {
            mark_old(ReceiverReprFamily::Proxy);
        }
    }
    if crate::timer::is_known_timer_id(addr as i64) {
        mark_old(ReceiverReprFamily::Timer);
    }
    if addr as i64 == crate::text::TEXT_ENCODER_SENTINEL_ID
        || crate::text::is_known_text_decoder_id(addr as i64)
    {
        mark_old(ReceiverReprFamily::Text);
    }
    if crate::tui::is_known_handle(addr as i64) {
        mark_old(ReceiverReprFamily::Tui);
    }
    if crate::async_hooks::is_async_hook_handle(addr as i64) {
        mark_old(ReceiverReprFamily::AsyncHook);
    }
    if crate::async_hooks::is_async_resource_handle(addr as i64) {
        mark_old(ReceiverReprFamily::AsyncResource);
    }
    if crate::object::is_null_stub_address(addr) {
        mark_old(ReceiverReprFamily::NullStub);
    }
    if crate::shared_sab::is_shared_sab(addr) {
        mark_old(ReceiverReprFamily::Sab);
    }

    let tracked = unsafe { crate::value::addr_class::try_read_tracked_gc_header(addr) };
    if tracked.is_none() {
        if crate::symbol::is_registered_symbol(addr) {
            mark_old(ReceiverReprFamily::SymbolGlobal);
        }
        if crate::buffer::is_external_buffer(addr) {
            mark_old(ReceiverReprFamily::ExternalBuffer);
        }
        return;
    }

    // Debug-only trust-the-tag audit. The ownership-derived header makes the
    // direct byte readable; release builds carry no direct-load probe at all.
    #[cfg(debug_assertions)]
    unsafe {
        let derived = tracked.unwrap();
        let derived_ptr = derived.as_ptr() as *const crate::gc::GcHeader;
        let direct =
            (addr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if direct != derived_ptr || (*direct).obj_type != (*derived_ptr).obj_type {
            DIRECT_MISMATCH.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[inline]
fn maybe_dump() {
    let event = EVENTS.fetch_add(1, Ordering::Relaxed);
    if event != 0 && event % SNAPSHOT_EVERY != 0 {
        return;
    }
    let mut last = LAST_DUMP.lock().unwrap();
    if last.is_some_and(|instant| instant.elapsed() < Duration::from_secs(1)) {
        return;
    }
    *last = Some(Instant::now());
    if let Some(sink) = receiver_repr_sink() {
        write_sink(sink, &render());
    }
}

fn append_family_counts(out: &mut String, values: &[AtomicU64; FAMILY_COUNT]) {
    for (index, name) in ReceiverReprFamily::NAMES.iter().enumerate() {
        if index != 0 {
            out.push(' ');
        }
        let _ = write!(out, "{name}={}", values[index].load(Ordering::Relaxed));
    }
}

fn render() -> String {
    let mut out = String::with_capacity(768);
    out.push_str("[receiver-repr-diag] constructed ");
    append_family_counts(&mut out, &COUNTERS.constructed);
    out.push_str("; observed_old ");
    append_family_counts(&mut out, &COUNTERS.observed_old);
    out.push_str("; observed_wrapped ");
    append_family_counts(&mut out, &COUNTERS.observed_wrapped);
    let _ = writeln!(
        out,
        "; bare_managed={}; invalid_pointer_zero={}; direct_mismatch={}",
        BARE_MANAGED.load(Ordering::Relaxed),
        INVALID_POINTER_ZERO.load(Ordering::Relaxed),
        DIRECT_MISMATCH.load(Ordering::Relaxed),
    );
    out
}

#[cfg(test)]
#[inline]
fn test_note_classification_entry() {
    TEST_CLASSIFICATION_ENTRIES.fetch_add(1, Ordering::Relaxed);
}

#[cfg(not(test))]
#[inline]
fn test_note_classification_entry() {}

#[cfg(test)]
pub(crate) fn receiver_repr_test_arm(armed: bool) {
    receiver_repr_sink();
    RECEIVER_REPR_ON.store(armed, Ordering::Relaxed);
    TEST_FORCE_ON.store(armed, Ordering::Relaxed);
}

#[cfg(test)]
pub(crate) fn receiver_repr_test_classification_entries() -> u64 {
    TEST_CLASSIFICATION_ENTRIES.load(Ordering::Relaxed)
}

#[cfg(test)]
pub(crate) fn receiver_repr_test_reset() {
    for family in ReceiverReprFamily::ALL {
        COUNTERS.constructed[family.index()].store(0, Ordering::Relaxed);
        COUNTERS.observed_old[family.index()].store(0, Ordering::Relaxed);
        COUNTERS.observed_wrapped[family.index()].store(0, Ordering::Relaxed);
    }
    BARE_MANAGED.store(0, Ordering::Relaxed);
    INVALID_POINTER_ZERO.store(0, Ordering::Relaxed);
    DIRECT_MISMATCH.store(0, Ordering::Relaxed);
    EVENTS.store(0, Ordering::Relaxed);
    TEST_CLASSIFICATION_ENTRIES.store(0, Ordering::Relaxed);
    *LAST_DUMP.lock().unwrap() = None;
}

#[cfg(test)]
pub(crate) fn receiver_repr_test_snapshot(family: ReceiverReprFamily) -> (u64, u64, u64) {
    let index = family.index();
    (
        COUNTERS.constructed[index].load(Ordering::Relaxed),
        COUNTERS.observed_old[index].load(Ordering::Relaxed),
        COUNTERS.observed_wrapped[index].load(Ordering::Relaxed),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn assert_fixture(family: ReceiverReprFamily, construct: impl FnOnce() -> (usize, bool)) {
        receiver_repr_test_reset();
        receiver_repr_test_arm(true);
        let (value, already_nanboxed) = construct();
        if already_nanboxed {
            receiver_repr_note_value(f64::from_bits(value as u64));
        } else {
            receiver_repr_note_decoded_pointer(value);
        }
        let (constructed, observed, wrapped) = receiver_repr_test_snapshot(family);
        assert!(
            constructed > 0,
            "{family:?} constructor did not move its bucket"
        );
        assert!(
            observed > 0,
            "{family:?} receiver did not move observed_old"
        );
        assert_eq!(wrapped, 0, "PR 1 must not create wrappers");
    }

    #[test]
    fn receiver_repr_family_fixtures_move_constructed_and_observed_old() {
        // These three producers live in perry-stdlib, below perry-runtime in
        // the dependency graph. Their exact producer calls are pinned by the
        // source-witness test below; the fixtures exercise their audited bands.
        assert_fixture(ReceiverReprFamily::Common, || {
            receiver_repr_note_constructed(ReceiverReprFamily::Common);
            (2, false)
        });
        assert_fixture(ReceiverReprFamily::Fetch, || {
            receiver_repr_note_constructed(ReceiverReprFamily::Fetch);
            (crate::value::addr_class::FETCH_HANDLE_BAND_START, false)
        });
        assert_fixture(ReceiverReprFamily::Zlib, || {
            receiver_repr_note_constructed(ReceiverReprFamily::Zlib);
            (crate::value::addr_class::ZLIB_HANDLE_BAND_START, false)
        });
        assert_fixture(ReceiverReprFamily::Proxy, || {
            let object = || {
                let ptr = crate::object::js_object_alloc(0, 0);
                f64::from_bits(crate::value::JSValue::pointer(ptr.cast()).bits())
            };
            (
                crate::proxy::js_proxy_new(object(), object()).to_bits() as usize,
                true,
            )
        });
        assert_fixture(ReceiverReprFamily::Timer, || {
            (
                crate::timer::js_set_timeout_callback(0, 60_000.0) as usize,
                false,
            )
        });
        assert_fixture(ReceiverReprFamily::Text, || {
            (crate::text::js_text_encoder_new() as usize, false)
        });
        assert_fixture(ReceiverReprFamily::Tui, || {
            let mut handle = crate::tui::state::js_perry_tui_state_alloc(0.0);
            if handle == 0 {
                handle = crate::tui::state::js_perry_tui_state_alloc(0.0);
            }
            (handle as usize, false)
        });
        assert_fixture(ReceiverReprFamily::AsyncHook, || {
            let options = crate::object::js_object_alloc(0, 0);
            let value = f64::from_bits(crate::value::JSValue::pointer(options.cast()).bits());
            (
                crate::async_hooks::js_async_hooks_create_hook(value) as usize,
                false,
            )
        });
        assert_fixture(ReceiverReprFamily::AsyncResource, || {
            let name = crate::string::js_string_from_bytes(b"receiver-repr".as_ptr(), 13);
            let type_value = f64::from_bits(crate::value::js_nanbox_string(name as i64).to_bits());
            let options = f64::from_bits(crate::value::TAG_UNDEFINED);
            (
                crate::async_hooks::js_async_resource_new(type_value, options) as usize,
                false,
            )
        });
        assert_fixture(ReceiverReprFamily::SymbolGlobal, || {
            (
                crate::symbol::well_known_symbol("receiverReprFixture") as usize,
                false,
            )
        });
        assert_fixture(ReceiverReprFamily::ExternalBuffer, || {
            let buffer = Box::into_raw(Box::new(crate::buffer::BufferHeader {
                length: 0,
                capacity: 0,
            }));
            crate::buffer::js_buffer_register_external(buffer as usize);
            (buffer as usize, false)
        });
        assert_fixture(ReceiverReprFamily::Sab, || {
            (crate::shared_sab::alloc_shared_sab(1) as usize, false)
        });
        assert_fixture(ReceiverReprFamily::NullStub, || {
            (
                crate::object::js_unresolved_namespace_stub().to_bits() as usize,
                true,
            )
        });

        let line = render();
        assert!(line.starts_with("[receiver-repr-diag] constructed common=0"));
        assert!(line.contains("null_stub=1; observed_old"));
        assert!(line.contains("null_stub=1; observed_wrapped"));
        assert!(line.ends_with("bare_managed=0; invalid_pointer_zero=0; direct_mismatch=0\n"));
        receiver_repr_test_arm(false);
    }

    /// Source witnesses bind the fixture above to every audited producer. If a
    /// producer-side bump is dropped, its exact occurrence floor fails here.
    #[test]
    fn receiver_repr_every_family_producer_keeps_its_constructed_bump() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let workspace = manifest.parent().and_then(Path::parent).unwrap();
        let witnesses = [
            ("crates/perry-stdlib/src/common/handle.rs", "Common", 2),
            ("crates/perry-stdlib/src/fetch/mod.rs", "Fetch", 1),
            ("crates/perry-stdlib/src/zlib.rs", "Zlib", 1),
            ("crates/perry-runtime/src/proxy.rs", "Proxy", 1),
            ("crates/perry-runtime/src/timer.rs", "Timer", 1),
            ("crates/perry-runtime/src/text.rs", "Text", 2),
            ("crates/perry-runtime/src/tui/tree.rs", "Tui", 1),
            ("crates/perry-runtime/src/tui/state.rs", "Tui", 1),
            ("crates/perry-runtime/src/tui/hooks.rs", "Tui", 4),
            ("crates/perry-runtime/src/async_hooks.rs", "AsyncHook", 1),
            (
                "crates/perry-runtime/src/async_hooks.rs",
                "AsyncResource",
                1,
            ),
            ("crates/perry-runtime/src/symbol.rs", "SymbolGlobal", 2),
            (
                "crates/perry-runtime/src/symbol/constructors.rs",
                "SymbolGlobal",
                1,
            ),
            (
                "crates/perry-runtime/src/buffer/header.rs",
                "ExternalBuffer",
                1,
            ),
            ("crates/perry-runtime/src/shared_sab.rs", "Sab", 1),
            (
                "crates/perry-runtime/src/object/null_stub.rs",
                "NullStub",
                1,
            ),
        ];
        for (relative, family, expected) in witnesses {
            let text = std::fs::read_to_string(workspace.join(relative)).unwrap();
            let needle = format!("ReceiverReprFamily::{family}");
            assert_eq!(
                text.matches(&needle).count(),
                expected,
                "producer-side diagnostic bump changed in {relative}"
            );
        }
    }
}
