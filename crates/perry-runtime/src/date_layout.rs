//! Common Date cell representation, identity and timestamp access.
pub(crate) const NANBOX_PTR_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
/// A `Date` is a **reference type**: a NaN-boxed POINTER (POINTER_TAG) to
/// this 1-slot mutable heap cell, *not* a raw f64 timestamp. Aliases
/// (`const b = a`), function parameters, and closure captures all share
/// the same cell, so an in-place setter mutation is visible through every
/// binding — this is what fixes #2089 (effect `DateTime.add`). The cell is
/// arena-allocated as `GC_TYPE_DATE_CELL`: non-movable (a NaN-boxed pointer
/// living in a plain f64/DOUBLE local is kept alive by the conservative
/// stack scan, and the stable address means that un-shadow-rooted pointer
/// never goes stale across a GC).
///
/// RULE 3 (single-path object model, `object/shape_rule3.rs`): `meta` comes
/// FIRST, and the timestamp starts at payload `+8`. The emitted property-read
/// path loads the u32 at `receiver + 4` and compares it against a cached
/// ShapeId; with `ts` at `+0` that word was the high half of an IEEE double,
/// and `new Date(-1)` — `0xBFF0_0000_0000_0000` — put `0xBFF0_0000` there,
/// inside `[SHAPE_ID_BASE, SHAPE_ID_END)`. A time value has no bound to
/// appeal to, so the cell is relaid out instead: `+4` is now the high half of
/// a heap pointer (zero while `meta` is null), which the collector's own
/// address ceiling keeps below `0x1_0000`. Field order is the only thing that
/// changed — the cell is still two words, and nothing outside this file reads
/// it by offset (`object::cell_meta_slot` and `thread.rs` both go through the
/// named fields).
#[repr(C)]
pub struct DateCell {
    /// #6759 phase 1 (header unification): per-object metadata record, or null.
    ///
    /// Adding this made `DateCell` non-pointer-free, so its GC type entry moved
    /// from `GcRewriteDescriptorKind::Leaf` (a no-op arm) to `MetaOnly` and its
    /// `pointer_free` flag to `false` — a cell with a pointer must be scanned.
    pub meta: *mut crate::object::ObjectMeta,
    /// ILP32: the four bytes between the pointer and the eight-byte-aligned
    /// timestamp. They are the word at payload `+4` that every property read
    /// loads as a ShapeId, so `alloc_date_cell` must zero them (64-bit targets
    /// have the pointer's high half there, which is zero already).
    #[cfg(target_pointer_width = "32")]
    pub(crate) _meta_padding: u32,
    /// Milliseconds since the epoch, or NaN for an *Invalid Date*. A raw IEEE
    /// double, so a setter's store needs no write barrier.
    pub ts: f64,
}

const _: () = {
    assert!(std::mem::offset_of!(DateCell, meta) == 0);
    // RULE 3: the word at payload +4 must not be able to hold a ShapeId, so
    // the timestamp may not start before +8.
    assert!(std::mem::offset_of!(DateCell, ts) == 8);
    assert!(std::mem::size_of::<DateCell>() == 16);
};

/// True if `addr` (a cleaned heap address, NOT NaN-boxed bits) points at a
/// `DateCell`. Reads the `GcHeader.obj_type`: for any live `is_pointer()`
/// value the header is always present and valid, and because `DateCell` is
/// non-movable its address is stable, so this is an exact identity check
/// with no side-table registry to keep in sync.
#[inline]
pub fn is_date_cell_addr(addr: usize) -> bool {
    // #4004: small-handle registry ids (Web Fetch Request/Headers/Response,
    // perry-ffi/node:http handles, timer ids, …) are NaN-boxed as POINTER_TAG
    // values but are NOT real heap addresses — they live in the small-handle
    // band (see `value::addr_class`). Real `DateCell`s are arena-allocated,
    // always above the small-handle cutoff. Dereferencing
    // `addr - GC_HEADER_SIZE` on a small handle reads unmapped memory: once
    // #4018 moved fetch handles up to 0x40000 (past the old 0x1000 floor), any
    // untyped `request.headers.get()` dispatch routed its receiver through
    // `is_date_value` here and segfaulted. `try_read_gc_header` rejects the
    // whole band before the deref, so this is an exact heap-pointer check.
    // #5226: small typed arrays and `Buffer`s are raw-`alloc`'d off the GC
    // heap with NO `GcHeader` prefix. `try_read_gc_header` only rejects the
    // handle band and (on some platforms) low addresses, so on Windows these
    // pass its magnitude check and the `addr - GC_HEADER_SIZE` back-read below
    // faults when the block sits at the start of a freshly mapped region.
    // Reject them via the side tables first — they are never `DateCell`s.
    if crate::typedarray::is_offheap_sidetable_alloc(addr) {
        return false;
    }
    unsafe {
        let header = match crate::value::addr_class::try_read_gc_header(addr) {
            Some(header) => header,
            None => return false,
        };
        if header.obj_type != crate::gc::GC_TYPE_DATE_CELL {
            return false;
        }
        // #4003: `Buffer`s are raw-`alloc`'d with NO `GcHeader`, so the word at
        // `addr - GC_HEADER_SIZE` is unrelated heap memory that can
        // coincidentally hold the `DATE_CELL` tag — observed for the first
        // `gunzipSync` result, whose `.length`/`.byteLength` then mis-routed
        // through the Date branch in `js_object_get_field_by_name` and returned
        // `undefined`. A registered buffer is never a `DateCell`, so reject it.
        // The lookup runs only in the rare tag-match case, keeping the common
        // (non-Date) property-read path unchanged.
        if crate::buffer::is_registered_buffer(addr) {
            return false;
        }
        // #11558: a `Symbol.for` / well-known symbol is a Box-leaked
        // `SymbolHeader` with no `GcHeader` either, and under mimalloc the
        // word before it is the tail of the previous block — for a symbol
        // allocated right after another symbol, that symbol's `id`. Once the
        // id counter passes 17 (`GC_TYPE_DATE_CELL`) the next symbol read as
        // a Date, and since `DateCell.meta` is the first word, its
        // `SYMBOL_MAGIC` word was then dereferenced as an `ObjectMeta`:
        // claude-code 2.1.112's `doctor` SIGSEGV'd reading `defaultProps` off
        // a React element-type symbol. Every symbol carries `SYMBOL_MAGIC` in
        // its first word, so the screen is one load for a real Date.
        // A real Date starts with a null or aligned ObjectMeta pointer; the
        // odd SYMBOL_MAGIC cannot occur there, so no address registry is needed.
        !crate::symbol::may_be_symbol_header(addr as *const u8)
    }
}

/// True if `value` is a Date — i.e. a NaN-boxed pointer to a `DateCell`.
/// Replaces the old value-keyed `is_registered_date_bits`.
#[inline]
pub fn is_date_value(value: f64) -> bool {
    let bits = value.to_bits();
    if !crate::value::JSValue::from_bits(bits).is_pointer() {
        return false;
    }
    is_date_cell_addr((bits & NANBOX_PTR_MASK) as usize)
}

/// Back-compat shim for the old value-keyed identity check. Date is now a
/// reference type, so identity is "the value is a pointer to a `DateCell`".
/// Kept so external callers (e.g. `perry-stdlib`'s querystring) need only a
/// trivial update. Prefer [`is_date_value`].
#[inline]
pub fn is_registered_date_bits(bits: u64) -> bool {
    is_date_value(f64::from_bits(bits))
}

/// Read the timestamp out of a Date value. If `value` is a `DateCell`
/// pointer, dereference it; otherwise (a raw numeric timestamp handed to a
/// Date method directly — e.g. via `Date.UTC()` or a legacy path) pass it
/// through unchanged. Every public Date getter/formatter funnels its
/// receiver through this so it works whether it was given a cell or a bare
/// number.
#[inline]
pub fn date_cell_timestamp(value: f64) -> f64 {
    let bits = value.to_bits();
    if crate::value::JSValue::from_bits(bits).is_pointer() {
        let addr = (bits & NANBOX_PTR_MASK) as usize;
        if is_date_cell_addr(addr) {
            return unsafe { (*(addr as *const DateCell)).ts };
        }
    }
    value
}
