//! A cached heap value survives — and is rewritten across — a copying
//! minor collection.
//!
//! # Why this is its own test binary
//!
//! The assertion that matters here is that the collector *moved* the
//! cached string and the root scanner rewrote the cache's slot to the
//! forwarded address. Marking alone, or a non-moving collection, satisfies
//! "the value is still readable" without exercising one line of the
//! rewrite path — the #6942/#6946 failure mode, where a gate is green
//! because its subject never ran.
//!
//! Making that assertion meaningful requires a clean heap, and a shared
//! unit-test binary cannot provide one:
//!
//! - The collector conservatively pins any nursery object some stack word
//!   happens to point at. Run after even one other test in the same
//!   binary, this string gets pinned and the minor reports
//!   `copied_objects=0` (measured; alone it reports `copied_objects=1`),
//!   so the relocation assertion fails through no fault of the binding.
//! - Worse, driving `gc_collect_minor()` from a unit-test binary that has
//!   also allocated JS *objects* (which the constructor tests must do, to
//!   pass a real options object) makes the copying collector walk a bogus
//!   slot address and SIGSEGV in `gc::copying::scan_slot`. That
//!   reproduces deterministically under `--test-threads=1` and is a
//!   runtime-level fragility, not something this binding controls.
//!
//! A dedicated integration binary gets a pristine process: an empty
//! nursery, a shallow stack, and no JS objects. The relocation then
//! happens every run, so the assertion below is a real gate.
//!
//! Note the coverage trade-off: per-PR CI runs `cargo test --lib --bins`,
//! so this suite runs on PRs that touch it (via `e2e-scoped`), on nightly
//! and on tags — not on every PR. That is the price of an assertion that
//! can actually fail; a version of this test that lived in the unit
//! binary could only keep passing while covering nothing.

use perry_ext_lru_cache::{js_lru_cache_get, js_lru_cache_new, js_lru_cache_set};
use perry_ffi::{
    alloc_string, nanbox_string_bits, read_bytes, Handle, JsString, JsValue, ObjectHeader,
    StringHeader,
};

const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

extern "C" {
    fn js_object_alloc(class_id: u32, field_count: u32) -> *mut ObjectHeader;
    fn js_object_set_field_by_name(obj: *mut ObjectHeader, key: *const StringHeader, value: f64);
    fn js_get_string_pointer_unified(value: f64) -> i64;
}

/// `{ max: <max> }` as a real JS object, the way a compiled object
/// literal reaches the constructor.
fn options_with_max(max: f64) -> f64 {
    let ptr = unsafe { js_object_alloc(0, 1) };
    assert!(!ptr.is_null(), "js_object_alloc returned null");
    let key = alloc_string("max");
    unsafe { js_object_set_field_by_name(ptr, key.as_raw(), max) };
    f64::from_bits(JsValue::from_object_ptr(ptr).bits())
}

fn string_value(text: &str) -> f64 {
    let s = alloc_string(text);
    assert!(!s.is_null(), "alloc_string returned null");
    f64::from_bits(nanbox_string_bits(s.as_raw()))
}

/// Folded into the address so no stack word held across the collection
/// looks like a heap pointer.
const ADDRESS_TOKEN_XOR: u64 = 0xA5A5_A5A5_A5A5_A5A5;

/// Allocate the value, hand it to the cache, and return **only** an
/// obfuscated token for the address it started at.
///
/// `#[inline(never)]` is load-bearing, and so is the fact that neither the
/// raw address nor the NaN-boxed string ever becomes a local of the
/// caller. The collector conservatively pins any nursery object that some
/// live stack word points at — that is precisely the effect this test had
/// to be moved into its own binary to escape. A caller-frame local holding
/// the bare address would pin the very string whose relocation is being
/// asserted, and the assertion would then fail while proving nothing about
/// the cache-root rewrite. Keeping the pointer-shaped values inside a
/// frame that is popped before `gc_collect_minor()` runs, and carrying
/// only `address ^ ADDRESS_TOKEN_XOR` across it, removes that hazard.
#[inline(never)]
fn insert_value_and_take_address_token(handle: Handle) -> u64 {
    let value = string_value("value-object-1234567890");
    let address = value.to_bits() & POINTER_MASK;
    assert!(
        perry_runtime::arena::pointer_in_nursery(address as usize),
        "the value must start in the nursery or a minor cannot move it"
    );
    js_lru_cache_set(handle, 1.0, value);
    address ^ ADDRESS_TOKEN_XOR
}

fn read_string_value(value: f64) -> Option<String> {
    let ptr = unsafe { js_get_string_pointer_unified(value) } as *mut StringHeader;
    if ptr.is_null() {
        return None;
    }
    let handle = unsafe { JsString::from_raw(ptr) };
    read_bytes(handle).map(|b| String::from_utf8_lossy(b).into_owned())
}

#[test]
fn cached_value_survives_and_is_rewritten_by_a_copying_minor() {
    // Match the runtime's evacuation preconditions: write barriers active
    // and a live shadow frame (mirrors perry-ext-events' scanner test).
    perry_runtime::gc::js_gc_write_barriers_emitted(1);
    let frame = perry_runtime::gc::js_shadow_frame_push(0);
    // Both are process-global. A failing assertion below must not leave
    // barriers on and a frame pushed, so tear down in `Drop` rather than
    // on the happy path.
    struct GcStateGuard(u64);
    impl Drop for GcStateGuard {
        fn drop(&mut self) {
            perry_runtime::gc::js_shadow_frame_pop(self.0);
            perry_runtime::gc::js_gc_write_barriers_emitted(0);
        }
    }
    let _gc_state = GcStateGuard(frame);

    let h = js_lru_cache_new(options_with_max(10.0));
    // A >5-byte string forces the heap `StringHeader` repr (not inline
    // SSO), so it is a real collectable allocation. Its ONLY root is the
    // cache — nothing on the stack or in a global refers to it, which is
    // why the address is carried across the collection as an obfuscated
    // token rather than a live pointer-shaped word.
    let before_token = insert_value_and_take_address_token(h);

    // Reclaims unrooted nursery allocations and evacuates rooted survivors.
    let _ = perry_runtime::gc::gc_collect_minor();

    let got = js_lru_cache_get(h, 1.0);
    assert_ne!(
        got.to_bits(),
        TAG_UNDEFINED,
        "cached value was collected — the root scanner did not keep it alive"
    );

    // Subject-live gate. If the address did not change, the minor did not
    // copy anything and this test proved only that marking works — the
    // forwarding-pointer rewrite, which is the half most likely to break,
    // went untested. `PERRY_GEN_GC=0` routes `gc_collect_minor` to
    // non-moving mark-sweep and will trip this deliberately.
    let after = got.to_bits() & POINTER_MASK;
    // Safe to materialize now: the collection is over, so a pointer-shaped
    // stack word can no longer pin anything.
    let before = before_token ^ ADDRESS_TOKEN_XOR;
    assert_ne!(
        before, after,
        "minor GC did not relocate the cached value (0x{before:x}), so this run \
         never exercised the scanner's forwarding-pointer rewrite"
    );
    assert_eq!(
        read_string_value(got).as_deref(),
        Some("value-object-1234567890"),
        "cached value corrupted across GC — the slot was not rewritten to the \
         forwarded address"
    );

    perry_ffi::drop_handle(h);
}
