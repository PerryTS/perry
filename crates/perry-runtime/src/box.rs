//! Box runtime for mutable captured variables
//!
//! When a closure captures a variable that is modified (either in the closure
//! or in the outer scope), we need to store it in a heap-allocated "box" so
//! both scopes share the same storage location.

use std::alloc::{alloc, Layout};
use std::sync::atomic::{AtomicU64, Ordering};

static BOX_GET_NULL_COUNT: AtomicU64 = AtomicU64::new(0);
static BOX_SET_NULL_COUNT: AtomicU64 = AtomicU64::new(0);
static I32_BOX_GET_NULL_COUNT: AtomicU64 = AtomicU64::new(0);
static I32_BOX_SET_NULL_COUNT: AtomicU64 = AtomicU64::new(0);
static BOOL_BOX_GET_NULL_COUNT: AtomicU64 = AtomicU64::new(0);
static BOOL_BOX_SET_NULL_COUNT: AtomicU64 = AtomicU64::new(0);

/// A box is simply a heap-allocated JSValue bit slot.
#[repr(C)]
pub struct Box {
    pub value: u64,
}

#[repr(C, align(8))]
pub struct I32Box {
    pub value: i32,
}

#[repr(C, align(8))]
pub struct BoolBox {
    pub value: bool,
}

thread_local! {
    /// Registry of every active box pointer. GC traces the contained
    /// JSValue bits so that NaN-boxed heap pointers stored in boxes (e.g.
    /// the generator state machine's iter object held in `__iter`'s
    /// mutable-capture box) keep the referenced heap object alive
    /// across collections. Without this, captures stored as raw box
    /// pointers in closure capture slots fail the `valid_ptrs.contains`
    /// check during `trace_closure` (boxes come from `std::alloc::alloc`
    /// directly, not the GC arena), so the box pointer is never marked
    /// AND the JSValue bits inside are never scanned — heap objects
    /// referenced only through box-captures can be swept mid-await.
    pub(crate) static BOX_REGISTRY: std::cell::RefCell<crate::fast_hash::PtrHashSet<usize>> =
        // Pre-size for promise-heavy workloads: `promise_all_chains`
        // allocates ~150 k boxes per kernel run (one per closure
        // mutable capture). Starting at 128 k buckets (~2 MB) covers
        // the full working set in one alloc — without it, hashbrown
        // rehashes from 0 → 256 k buckets across the alloc history,
        // showing up as ~3 % CPU in `hash_one` / `reserve_rehash`.
        std::cell::RefCell::new(std::collections::HashSet::with_capacity_and_hasher(
            128 * 1024,
            crate::fast_hash::PtrHasher,
        ));
    pub(crate) static I32_BOX_REGISTRY: std::cell::RefCell<crate::fast_hash::PtrHashSet<usize>> =
        std::cell::RefCell::new(std::collections::HashSet::with_capacity_and_hasher(
            16 * 1024,
            crate::fast_hash::PtrHasher,
        ));
    pub(crate) static BOOL_BOX_REGISTRY: std::cell::RefCell<crate::fast_hash::PtrHashSet<usize>> =
        std::cell::RefCell::new(std::collections::HashSet::with_capacity_and_hasher(
            16 * 1024,
            crate::fast_hash::PtrHasher,
        ));
}

/// Allocate a new box with an initial JSValue bit pattern.
#[no_mangle]
pub extern "C" fn js_box_alloc_bits(initial_bits: i64) -> *mut Box {
    unsafe {
        let layout = Layout::new::<Box>();
        let ptr = alloc(layout) as *mut Box;
        if ptr.is_null() {
            // perry#924: oom is rare enough that operators see the
            // downstream crash and react to that; keep the diagnostic
            // available under `PERRY_DEBUG=1` for bisection.
            if std::env::var_os("PERRY_DEBUG").is_some() {
                eprintln!("[PERRY WARN] js_box_alloc: allocation failed — returning null");
            }
            return std::ptr::null_mut();
        }
        (*ptr).value = initial_bits as u64;
        BOX_REGISTRY.with(|r| {
            r.borrow_mut().insert(ptr as usize);
        });
        ptr
    }
}

/// Compatibility wrapper for legacy f64-lowered boxed locals.
#[no_mangle]
pub extern "C" fn js_box_alloc(initial_value: f64) -> *mut Box {
    js_box_alloc_bits(initial_value.to_bits() as i64)
}

#[no_mangle]
pub extern "C" fn js_i32_box_alloc(initial_value: i32) -> *mut I32Box {
    unsafe {
        let layout = Layout::new::<I32Box>();
        let ptr = alloc(layout) as *mut I32Box;
        if ptr.is_null() {
            if std::env::var_os("PERRY_DEBUG").is_some() {
                eprintln!("[PERRY WARN] js_i32_box_alloc: allocation failed — returning null");
            }
            return std::ptr::null_mut();
        }
        (*ptr).value = initial_value;
        I32_BOX_REGISTRY.with(|r| {
            r.borrow_mut().insert(ptr as usize);
        });
        ptr
    }
}

#[no_mangle]
pub extern "C" fn js_bool_box_alloc(initial_value: i32) -> *mut BoolBox {
    unsafe {
        let layout = Layout::new::<BoolBox>();
        let ptr = alloc(layout) as *mut BoolBox;
        if ptr.is_null() {
            if std::env::var_os("PERRY_DEBUG").is_some() {
                eprintln!("[PERRY WARN] js_bool_box_alloc: allocation failed — returning null");
            }
            return std::ptr::null_mut();
        }
        (*ptr).value = initial_value != 0;
        BOOL_BOX_REGISTRY.with(|r| {
            r.borrow_mut().insert(ptr as usize);
        });
        ptr
    }
}

/// GC root scanner: walk every registered box and `mark` the JSValue bit
/// value inside. Heap pointers stored inside boxes (e.g. the generator
/// state machine's iter object held in a mutable-capture box) must be
/// kept alive across collections. The box pointer itself is _not_ a
/// heap value the runtime tracks — `BOX_REGISTRY` is the source of
/// truth for "every live box right now" — so we use the standard root
/// scanner protocol: dispatch every stored JSValue bit pattern to `mark`
/// and let the GC trace into it.
pub fn scan_box_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_box_roots_mut(&mut visitor);
}

pub fn scan_box_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    BOX_REGISTRY.with(|r| {
        let r = r.borrow();
        for &addr in r.iter() {
            let ptr = addr as *mut Box;
            // Defensive: the registry should only contain valid live
            // pointers, but if a stale entry slipped through we'd
            // segfault on the deref. The tight bounds check on the
            // address (alloc gives 8-aligned pointers in user space)
            // matches `is_plausible_box_ptr` to keep this a no-op for
            // any pathological entry.
            if addr >= 0x1000 && (addr as u64) < 0x0001_0000_0000_0000 && addr % 8 == 0 {
                unsafe {
                    visitor.visit_nanbox_u64_raw_slot(&raw mut (*ptr).value);
                }
            }
        }
    });
}

/// Get the raw JSValue bit pattern from a box.
///
/// Same robustness as `js_box_set`: invalid pointers return `undefined`
/// rather than dereferencing. See perry#393 for the failure mode.
#[no_mangle]
pub extern "C" fn js_box_get_bits(ptr: *mut Box) -> i64 {
    unsafe {
        if !is_registered_box_ptr(ptr) {
            // perry#924: production services see these in tight bursts of
            // 3 synced with normal request handling and the operator can't
            // tell whether anything is wrong. The path is correctness-safe
            // (we already return a defined value to the caller); gate the
            // diagnostic behind `PERRY_DEBUG=1` so it only surfaces during
            // bisection.
            if std::env::var_os("PERRY_DEBUG").is_some() {
                let count = BOX_GET_NULL_COUNT.fetch_add(1, Ordering::Relaxed);
                if count < 3 {
                    eprintln!(
                        "[PERRY WARN] js_box_get: invalid box pointer {:p} #{}",
                        ptr, count
                    );
                }
            }
            // perry#4926: with codegen entry-initializing boxed slots to
            // TAG_UNDEFINED, this arm is the read-before-initialization
            // path for a boxed variable — in JS that reads as `undefined`
            // (Perry has no TDZ), not as the number NaN. TAG_UNDEFINED is
            // itself a quiet-NaN bit pattern, so numeric consumers behave
            // exactly as before; JS-level checks (`typeof`, `== null`)
            // now see `undefined`.
            return crate::value::TAG_UNDEFINED as i64;
        }
        let bits = (*ptr).value;
        // Temporal Dead Zone: a lexical `let`/`const`/`class` box seeded with
        // the TAG_TDZ sentinel at scope entry throws a spec ReferenceError when
        // read before its declaration runs (which overwrites the sentinel with
        // a real value). TAG_TDZ is a reserved bit pattern no legitimate value
        // ever holds, so this branch is only ever taken on a genuine
        // read-before-initialization — making the check zero-regression for
        // every already-initialized box. The name is passed as `undefined`
        // because this choke point is name-agnostic (it serves direct,
        // closure-captured, and compound reads alike); the resulting message is
        // the spec-generic form.
        if bits == crate::value::TAG_TDZ {
            // #6044 regression (#6052): Perry-internal materialization reads —
            // the class-capture decl-site snapshot refreshes emitted after EACH
            // captured var's assignment (`RegisterClassCaptures`, the #6037
            // refresh strategy) — legally observe sibling captures that are
            // still in their dead zone (`const _fs = ..; <refresh reads _path>;
            // const _path = ..`, the SWC CJS interop shape). Those are not user
            // reads: pre-TDZ they snapshotted `undefined` and the next refresh
            // fixed the value up. Inside the codegen-bracketed suppression
            // window, keep exactly that behavior instead of throwing.
            if TDZ_SUPPRESS_DEPTH.with(|d| d.get()) > 0 {
                return crate::value::TAG_UNDEFINED as i64;
            }
            crate::error::js_throw_reference_error_tdz(f64::from_bits(crate::value::TAG_UNDEFINED));
        }
        bits as i64
    }
}

thread_local! {
    /// #6052: >0 while codegen-emitted Perry-internal materialization reads
    /// (the `RegisterClassCaptures` decl-site snapshot refresh) are running —
    /// a dead-zone box then reads as `undefined` (pre-#6044 behavior) instead
    /// of throwing. Never spans user code: the bracketed window contains only
    /// side-effect-free capture loads.
    static TDZ_SUPPRESS_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Enter a TDZ-suppression window (see `TDZ_SUPPRESS_DEPTH`). Emitted by
/// codegen immediately before a `RegisterClassCaptures` snapshot's capture
/// loads; paired with `js_tdz_suppress_end`.
#[no_mangle]
pub extern "C" fn js_tdz_suppress_begin() {
    TDZ_SUPPRESS_DEPTH.with(|d| d.set(d.get().saturating_add(1)));
}

/// Leave the TDZ-suppression window opened by `js_tdz_suppress_begin`.
#[no_mangle]
pub extern "C" fn js_tdz_suppress_end() {
    TDZ_SUPPRESS_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
}

/// Keepalive anchors for the auto-optimize whole-program build (generated-code-
/// only callees — without these the symbols dead-strip and the app link fails).
#[used]
static KEEP_JS_TDZ_SUPPRESS_BEGIN: extern "C" fn() = js_tdz_suppress_begin;
#[used]
static KEEP_JS_TDZ_SUPPRESS_END: extern "C" fn() = js_tdz_suppress_end;

/// Compatibility wrapper for legacy f64-lowered boxed locals.
#[no_mangle]
pub extern "C" fn js_box_get(ptr: *mut Box) -> f64 {
    f64::from_bits(js_box_get_bits(ptr) as u64)
}

#[no_mangle]
pub extern "C" fn js_i32_box_get(ptr: *mut I32Box) -> i32 {
    unsafe {
        if !is_registered_i32_box_ptr(ptr) {
            if std::env::var_os("PERRY_DEBUG").is_some() {
                let count = I32_BOX_GET_NULL_COUNT.fetch_add(1, Ordering::Relaxed);
                if count < 3 {
                    eprintln!(
                        "[PERRY WARN] js_i32_box_get: invalid box pointer {:p} #{}",
                        ptr, count
                    );
                }
            }
            return 0;
        }
        (*ptr).value
    }
}

#[no_mangle]
pub extern "C" fn js_bool_box_get(ptr: *mut BoolBox) -> i32 {
    unsafe {
        if !is_registered_bool_box_ptr(ptr) {
            if std::env::var_os("PERRY_DEBUG").is_some() {
                let count = BOOL_BOX_GET_NULL_COUNT.fetch_add(1, Ordering::Relaxed);
                if count < 3 {
                    eprintln!(
                        "[PERRY WARN] js_bool_box_get: invalid box pointer {:p} #{}",
                        ptr, count
                    );
                }
            }
            return 0;
        }
        i32::from((*ptr).value)
    }
}

/// Set the raw JSValue bit pattern in a box.
///
/// Robust against bogus pointers: in addition to the null check, we
/// reject obviously-invalid pointers (below the first user page or
/// above the 48-bit user-address ceiling) and pointers that aren't
/// 8-byte aligned. This avoids SIGSEGV on `(*ptr).value = value` when
/// upstream codegen hands us a stale/uninitialized slot — a known
/// failure mode for closure prologues at hub-scale (perry#393).
/// Boxes are heap-allocated 8-byte JSValue bit slots; a non-aligned or low/high
/// pointer is definitely wrong, so a silent skip + telemetry warning
/// is strictly safer than dereferencing it.
#[no_mangle]
pub extern "C" fn js_box_set_bits(ptr: *mut Box, value_bits: i64) {
    unsafe {
        if !is_registered_box_ptr(ptr) {
            // perry#924: silent-skip is correctness-safe (caller's box
            // mutation is dropped, which is the same as no closure
            // capture having existed). Gate diagnostics behind
            // `PERRY_DEBUG=1` to keep production stderr clean.
            if std::env::var_os("PERRY_DEBUG").is_some() {
                let count = BOX_SET_NULL_COUNT.fetch_add(1, Ordering::Relaxed);
                if count < 3 {
                    eprintln!(
                        "[PERRY WARN] js_box_set: invalid box pointer {:p} #{} (value bits: 0x{:016x})",
                        ptr,
                        count,
                        value_bits as u64
                    );
                }
            }
            return;
        }
        let bits = value_bits as u64;
        (*ptr).value = bits;
        crate::gc::runtime_write_barrier_root_nanbox(bits);
    }
}

/// Compatibility wrapper for legacy f64-lowered boxed locals.
#[no_mangle]
pub extern "C" fn js_box_set(ptr: *mut Box, value: f64) {
    js_box_set_bits(ptr, value.to_bits() as i64);
}

#[no_mangle]
pub extern "C" fn js_i32_box_set(ptr: *mut I32Box, value: i32) {
    unsafe {
        if !is_registered_i32_box_ptr(ptr) {
            if std::env::var_os("PERRY_DEBUG").is_some() {
                let count = I32_BOX_SET_NULL_COUNT.fetch_add(1, Ordering::Relaxed);
                if count < 3 {
                    eprintln!(
                        "[PERRY WARN] js_i32_box_set: invalid box pointer {:p} #{} (value: {})",
                        ptr, count, value
                    );
                }
            }
            return;
        }
        (*ptr).value = value;
    }
}

#[no_mangle]
pub extern "C" fn js_bool_box_set(ptr: *mut BoolBox, value: i32) {
    unsafe {
        if !is_registered_bool_box_ptr(ptr) {
            if std::env::var_os("PERRY_DEBUG").is_some() {
                let count = BOOL_BOX_SET_NULL_COUNT.fetch_add(1, Ordering::Relaxed);
                if count < 3 {
                    eprintln!(
                        "[PERRY WARN] js_bool_box_set: invalid box pointer {:p} #{} (value: {})",
                        ptr, count, value
                    );
                }
            }
            return;
        }
        (*ptr).value = value != 0;
    }
}

/// Cheap pointer-sanity test — same threat model as `get_valid_func_ptr`
/// in closure.rs, adapted for box-shaped allocations.
///
/// A `*mut Box` from `js_box_alloc` is a Rust-`alloc()` heap pointer,
/// which on x86_64 Linux/macOS lives in the 47-bit user-address half
/// of the address space and (because `Layout::new::<Box>()` yields
/// `align = 8`) is 8-byte aligned. Pointers below the first user page
/// or above the user-address ceiling, or unaligned ones, can only come
/// from stale/uninitialized stack slots reinterpreted as box pointers.
///
/// perry#4898: the structural checks are necessary but **not sufficient**.
/// A miscompiled `js_box_set` can be handed a box-pointer operand that was
/// effectively `undef`/poison at the IR level (e.g. a mutable-capture box
/// whose allocation was elided on the taken path). LLVM then fills the
/// register with whatever was conveniently live — under typed-feedback
/// (#854) instrumentation that is the read-only `..._guard` string constant
/// passed to `js_typed_feedback_register_site`. That constant is ≥0x1000,
/// untagged (top-16 zero), and 8-byte aligned, so it sails through every
/// structural check — and `(*ptr).value = value` then writes into
/// `__TEXT.__cstring`, a SIGBUS. The address `read_static`-looks like a box
/// but isn't one. `is_registered_box_ptr` closes that gap: a pointer that
/// `js_box_alloc` never minted is rejected before the deref.
#[inline]
fn is_plausible_box_ptr(ptr: *mut Box) -> bool {
    let addr = ptr as usize;
    if addr == 0 {
        return false;
    }
    if addr < 0x1000 {
        return false;
    }
    if (addr as u64) >= 0x0001_0000_0000_0000 {
        return false;
    }
    if !addr.is_multiple_of(std::mem::align_of::<Box>()) {
        return false;
    }
    true
}

/// Authoritative box-pointer check: the address must have been minted by
/// `js_box_alloc` (and thus recorded in `BOX_REGISTRY`). Boxes are never
/// freed — the registry is monotonic per thread — so membership has no
/// false negatives for a real live box and no stale-reuse hazard: an
/// address that isn't in the registry is provably not a box, regardless of
/// how plausible its bit-pattern looks. This is what stops a stray
/// read-only/garbage pointer (perry#4898) from being dereferenced as a box.
#[inline]
fn is_registered_box_ptr(ptr: *mut Box) -> bool {
    if !is_plausible_box_ptr(ptr) {
        return false;
    }
    BOX_REGISTRY.with(|r| r.borrow().contains(&(ptr as usize)))
}

#[inline]
fn is_registered_i32_box_ptr(ptr: *mut I32Box) -> bool {
    if !is_plausible_box_ptr(ptr.cast::<Box>()) {
        return false;
    }
    I32_BOX_REGISTRY.with(|r| r.borrow().contains(&(ptr as usize)))
}

#[inline]
fn is_registered_bool_box_ptr(ptr: *mut BoolBox) -> bool {
    if !is_plausible_box_ptr(ptr.cast::<Box>()) {
        return false;
    }
    BOOL_BOX_REGISTRY.with(|r| r.borrow().contains(&(ptr as usize)))
}

#[used]
static KEEP_JS_BOX_ALLOC_BITS: extern "C" fn(i64) -> *mut Box = js_box_alloc_bits;
#[used]
static KEEP_JS_BOX_GET_BITS: extern "C" fn(*mut Box) -> i64 = js_box_get_bits;
#[used]
static KEEP_JS_BOX_SET_BITS: extern "C" fn(*mut Box, i64) = js_box_set_bits;
#[used]
static KEEP_JS_BOX_ALLOC: extern "C" fn(f64) -> *mut Box = js_box_alloc;
#[used]
static KEEP_JS_BOX_GET: extern "C" fn(*mut Box) -> f64 = js_box_get;
#[used]
static KEEP_JS_BOX_SET: extern "C" fn(*mut Box, f64) = js_box_set;
#[used]
static KEEP_JS_I32_BOX_ALLOC: extern "C" fn(i32) -> *mut I32Box = js_i32_box_alloc;
#[used]
static KEEP_JS_I32_BOX_GET: extern "C" fn(*mut I32Box) -> i32 = js_i32_box_get;
#[used]
static KEEP_JS_I32_BOX_SET: extern "C" fn(*mut I32Box, i32) = js_i32_box_set;
#[used]
static KEEP_JS_BOOL_BOX_ALLOC: extern "C" fn(i32) -> *mut BoolBox = js_bool_box_alloc;
#[used]
static KEEP_JS_BOOL_BOX_GET: extern "C" fn(*mut BoolBox) -> i32 = js_bool_box_get;
#[used]
static KEEP_JS_BOOL_BOX_SET: extern "C" fn(*mut BoolBox, i32) = js_bool_box_set;

#[cfg(test)]
pub(crate) fn test_clear_box_registry() {
    BOX_REGISTRY.with(|r| r.borrow_mut().clear());
    I32_BOX_REGISTRY.with(|r| r.borrow_mut().clear());
    BOOL_BOX_REGISTRY.with(|r| r.borrow_mut().clear());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// perry#4898: a structurally-plausible pointer that `js_box_alloc`
    /// never minted (here, a `&'static` read-only constant that is ≥0x1000,
    /// untagged, and 8-byte aligned — exactly the shape of the leaked
    /// `..._guard` string) must NOT be dereferenced by `js_box_set`. Before
    /// the registry check this stored into read-only memory → SIGBUS.
    #[test]
    fn box_set_skips_unregistered_plausible_pointer() {
        test_clear_box_registry();
        // 8-byte aligned static — passes every structural check, is not a box.
        static RODATA: [u64; 2] = [0xDEAD_BEEF, 0xFEED_FACE];
        let fake = (&RODATA[0] as *const u64) as *mut Box;
        assert!(is_plausible_box_ptr(fake), "test needs a plausible ptr");
        assert!(!is_registered_box_ptr(fake), "fake must not be registered");
        // Must be a silent no-op, not a write/crash.
        js_box_set(fake, 1.0);
        js_box_set_bits(
            fake,
            crate::value::JSValue::try_short_string(b"bad")
                .unwrap()
                .bits() as i64,
        );
        assert_eq!(RODATA[0], 0xDEAD_BEEF, "rodata must be untouched");
        // Reads from an unregistered pointer return `undefined` (perry#4926:
        // the read-before-initialization value of a boxed variable), never
        // deref. TAG_UNDEFINED is a NaN bit pattern, so this also preserves
        // the older "returns NaN" numeric behavior.
        assert_eq!(
            js_box_get_bits(fake) as u64,
            crate::value::TAG_UNDEFINED,
            "unregistered bits box read must yield undefined"
        );
        assert_eq!(
            js_box_get(fake).to_bits(),
            crate::value::TAG_UNDEFINED,
            "unregistered box read must yield undefined"
        );
    }

    /// A real `js_box_alloc` box still round-trips through set/get after the
    /// registry gate (no false negatives on genuine boxes).
    #[test]
    fn box_set_get_roundtrips_for_real_box() {
        test_clear_box_registry();
        let b = js_box_alloc(3.5);
        assert!(is_registered_box_ptr(b));
        assert_eq!(js_box_get(b), 3.5);
        js_box_set(b, 42.0);
        assert_eq!(js_box_get(b), 42.0);
    }

    /// The bits ABI is the canonical boxed-local storage path for dynamic
    /// JSValues. It must not turn Perry's NaN-boxed non-number values into a
    /// numeric NaN payload.
    #[test]
    fn box_bits_roundtrips_non_number_tags_exactly() {
        test_clear_box_registry();
        let cases = [
            crate::value::JSValue::int32(-17).bits(),
            crate::value::JSValue::try_short_string(b"ok")
                .unwrap()
                .bits(),
            crate::value::TAG_UNDEFINED,
        ];

        for bits in cases {
            let b = js_box_alloc_bits(bits as i64);
            assert!(is_registered_box_ptr(b));
            assert_eq!(js_box_get_bits(b) as u64, bits);
            assert_eq!(js_box_get(b).to_bits(), bits);

            let replacement = crate::value::JSValue::try_short_string(b"next")
                .unwrap()
                .bits();
            js_box_set_bits(b, replacement as i64);
            assert_eq!(js_box_get_bits(b) as u64, replacement);
            assert_eq!(js_box_get(b).to_bits(), replacement);
        }
    }

    #[test]
    fn primitive_control_boxes_round_trip_and_reject_foreign_pointers() {
        test_clear_box_registry();
        let i32_box = js_i32_box_alloc(7);
        assert!(is_registered_i32_box_ptr(i32_box));
        assert_eq!(js_i32_box_get(i32_box), 7);
        js_i32_box_set(i32_box, -3);
        assert_eq!(js_i32_box_get(i32_box), -3);

        let bool_box = js_bool_box_alloc(0);
        assert!(is_registered_bool_box_ptr(bool_box));
        assert_eq!(js_bool_box_get(bool_box), 0);
        js_bool_box_set(bool_box, 1);
        assert_eq!(js_bool_box_get(bool_box), 1);

        let ordinary_box = js_box_alloc(1.0);
        assert_eq!(js_i32_box_get(ordinary_box.cast::<I32Box>()), 0);
        js_i32_box_set(ordinary_box.cast::<I32Box>(), 99);
        assert_eq!(js_box_get(ordinary_box), 1.0);
    }
}
