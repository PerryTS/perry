//! Scope context objects: one GC cell per activation of a group of
//! captured-and-mutated bindings (V8's `Context`, SpiderMonkey's
//! `CallObject`/`LexicalEnvironment`).
//!
//! Codegen groups the bindings of one lexical scope that the SAME set of
//! closures capture (`perry-codegen/src/scope_env`) and allocates one object
//! per activation of that group. Every binding is a NaN-boxed word at
//! `base + 8 * index`. The defining frame holds ONE root for the object and a
//! capturing closure holds ONE capture slot for it, instead of one of each per
//! binding: under RS4GC the relocation work is (#live GC pointers) ×
//! (#safepoints), so the root count is the cost.
//!
//! Every slot is an ordinary traced child edge (`GcRewriteDescriptorKind::
//! Scope`), rewritten on evacuation like a box value. Compiler-private async
//! control cells share the object: an i32 control word is stored in the low
//! half of a slot whose high half keeps `INT32_TAG`, and an i1 word in the low
//! byte of a slot whose high half keeps the special-constant tag, so no slot
//! ever holds a word the collector could mistake for a pointer.

use crate::gc::{GcHeader, GC_HEADER_SIZE, GC_TYPE_SCOPE};

/// Upper bound on one group's slot count. Codegen splits larger groups.
pub const SCOPE_MAX_SLOTS: usize = 4096;

/// Seed for a slot that holds a compiler-private i32 control word.
pub const SCOPE_I32_SLOT_SEED: u64 = 0x7FFE_0000_0000_0000;
/// Seed for a slot that holds a compiler-private i1 control word: the low byte
/// is the `bool` (0), the high half the non-pointer special-constant tag.
pub const SCOPE_I1_SLOT_SEED: u64 = 0x7FFC_0000_0000_0000;

/// Read-only `undefined` slots that an entry-cached capture base resolves to
/// when the capture word is not a scope object (see `js_scope_capture_base`).
/// The cache admits only bindings its body never writes, so nothing stores
/// here. Never a heap address: root enumeration ignores it.
static SCOPE_FALLBACK_SLOTS: [u64; SCOPE_MAX_SLOTS] =
    [crate::value::TAG_UNDEFINED; SCOPE_MAX_SLOTS];

/// A group root whose allocating statement did not run on this path (a sibling
/// branch of an async state-machine wrapper, a skipped hoisted declaration)
/// still holds its TAG_UNDEFINED entry sentinel. A per-binding cell in that
/// state read as `undefined` and dropped writes; the scope accessors keep that
/// behaviour. Every real base is a raw user pointer (high 16 bits clear).
#[inline(always)]
fn is_unallocated_base(base: i64) -> bool {
    (base as u64) >> 48 != 0
}

/// The number of slots `header` holds. Arena objects are padded to 8 bytes,
/// and `js_scope_alloc` seeds every word up to the header's size, so this is
/// the count the tracer may read.
///
/// # Safety
/// `header` must be the live header of a `GC_TYPE_SCOPE` object.
#[inline]
pub(crate) unsafe fn scope_slot_count(header: *const GcHeader) -> usize {
    unsafe { ((*header).size as usize).saturating_sub(GC_HEADER_SIZE) / 8 }
}

/// Allocate a scope object with `nslots` slots, each seeded with `seed`.
/// Returns the object's user pointer (raw, not NaN-boxed).
#[no_mangle]
pub extern "C" fn js_scope_alloc(nslots: i32, seed: i64) -> i64 {
    let n = (nslots.max(1) as usize).min(SCOPE_MAX_SLOTS);
    // `n * 8` is already 8-aligned, so the header's size is exactly
    // `GC_HEADER_SIZE + 8 * n` and these are all the words the tracer reads.
    let ptr = crate::arena::arena_alloc_gc(n * 8, 8, GC_TYPE_SCOPE);
    unsafe {
        let slots = ptr as *mut u64;
        for i in 0..n {
            // GC_STORE_AUDIT(INIT): seeding a fresh object with a non-pointer tag word.
            slots.add(i).write(seed as u64);
        }
    }
    ptr as i64
}

/// Store `bits` into slot `index` of the scope object at `base` and shade the
/// edge for the generational/incremental collector.
///
/// # Safety
/// `base` must be a live scope object with more than `index` slots, or an
/// unallocated root's NaN-boxed sentinel (the store is then dropped).
#[no_mangle]
pub unsafe extern "C" fn js_scope_set(base: i64, index: i32, bits: i64) {
    if is_unallocated_base(base) {
        return;
    }
    let slot = base as usize + 8 * index as usize;
    unsafe {
        // GC_STORE_AUDIT(BARRIERED): the slot barrier with the object as parent follows.
        (slot as *mut u64).write(bits as u64);
    }
    crate::gc::runtime_write_barrier_slot(base as usize, slot, bits as u64);
}

/// True when `bits` is the raw user pointer of a live scope object.
pub(crate) fn is_scope_ptr(bits: u64) -> bool {
    crate::r#box::has_cell_type(bits as usize, GC_TYPE_SCOPE, None)
}

/// A scope object's slot words, for thread transfer.
pub fn scope_slot_contents(bits: u64) -> Option<Vec<u64>> {
    if !is_scope_ptr(bits) {
        return None;
    }
    unsafe {
        let header = crate::value::addr_class::try_read_tracked_gc_header(bits as usize)?;
        let n = scope_slot_count(header.as_ptr());
        let slots = bits as usize as *const u64;
        Some((0..n).map(|i| slots.add(i).read()).collect())
    }
}

/// A captured cell word a trusted clone may read directly: a variable box or a
/// scope object.
pub fn is_capture_cell_ptr(bits: u64) -> bool {
    crate::r#box::box_slot_contents_bits(bits).is_some() || is_scope_ptr(bits)
}

/// Resolve a captured scope base for a cached read: the object itself when
/// valid, otherwise the fallback region (mirrors `js_box_capture_cell_ptr`).
#[no_mangle]
pub extern "C" fn js_scope_capture_base(bits: i64) -> i64 {
    if is_scope_ptr(bits as u64) {
        bits
    } else {
        SCOPE_FALLBACK_SLOTS.as_ptr() as i64
    }
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_SCOPE_ALLOC: extern "C" fn(i32, i64) -> i64 = js_scope_alloc;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_SCOPE_SET: unsafe extern "C" fn(i64, i32, i64) = js_scope_set;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_SCOPE_CAPTURE_BASE: extern "C" fn(i64) -> i64 = js_scope_capture_base;
