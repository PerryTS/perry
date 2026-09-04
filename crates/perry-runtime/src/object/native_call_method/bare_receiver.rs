//! #9675: canonicalizing a LEGACY BARE managed receiver at the dispatch
//! tower's entry, before it is rooted and before the first probe.
//!
//! A "bare" receiver is a real GC pointer whose value was never NaN-boxed, so
//! its top 16 bits are zero and it decodes as a positive subnormal double.
//! Perry still produces this shape — `gc_pointer_and_type_from_value` accepts
//! it explicitly — and `js_native_call_method` recovered it, but only in the
//! last few lines of the tower and only ever as `POINTER_TAG`. Three separate
//! defects followed from that, and this module closes all three by rewriting
//! the receiver into the tag it actually deserves *at the entry*.
//!
//! ## 1. The receiver was rooted under a tag the collector ignores
//!
//! The tower roots its receiver with
//! [`RuntimeHandleScope::root_nanbox_f64`](crate::gc::RuntimeHandleScope::root_nanbox_f64),
//! which parks it in a `RuntimeHandleSlot::Nanbox`. That slot kind is marked by
//! `gc::try_mark_value` and rewritten by `gc::try_rewrite_nanboxed_value`, and
//! **both begin by rejecting any word whose tag is not
//! `POINTER_TAG`/`STRING_TAG`/`BIGINT_TAG`**. A bare pointer's tag is zero, so
//! rooting one as a nanbox
//!
//! * does not MARK the receiver — a full mark-sweep inside dispatch can reap
//!   it, and
//! * does not REWRITE the slot — an evacuating minor inside dispatch leaves it
//!   holding a from-space address.
//!
//! #7528 already established that the tower runs "~1160 more lines across a
//! dozen probes that allocate" after that root, and made every use re-read the
//! root slot rather than a local copy. That fix is necessary but not
//! sufficient: re-reading a slot the collector never rewrote returns the same
//! stale address. The tail recovery then validates it with magnitude-only
//! predicates (`is_above_handle_band` + `is_valid_obj_ptr`), which cannot tell
//! a live address from a stale one, and dispatches on whatever now occupies
//! those bytes.
//!
//! Canonicalizing here means the value the tower roots carries a tag the
//! collector traces AND rewrites, so the receiver is kept alive and every
//! `object()` re-read below yields its current address. This is the runtime-side
//! form of the root-dominance invariant in
//! `docs/src/internals/gc-rooting-invariant.md`: the static checker reads
//! emitted LLVM IR and cannot see a mis-tagged runtime root at all.
//!
//! ## 2. Strings and BigInts came back as objects
//!
//! `JSValue::pointer` stamps `POINTER_TAG` unconditionally. `is_any_string()`
//! accepts only `STRING_TAG`/`SHORT_STRING_TAG`, and `string_methods::
//! dispatch_string` gates on exactly that predicate — so a bare `GC_TYPE_STRING`
//! receiver was reboxed into something that is not a string, `.slice` never
//! reached the string arm, and the call fell through to the `<m> is not a
//! function` catch-all on a perfectly good string. `.slice` on a value that
//! must be a string is the reported symptom of #9675. BigInt had the same
//! shape via `BIGINT_TAG`.
//!
//! ## 3. An already-forwarded receiver was dispatched at its old address
//!
//! The tail recovery did no forwarding walk, so a bare receiver that a
//! collection had already moved was reboxed at its from-space address.
//!
//! ## What the gate is, and what it deliberately leaves alone
//!
//! Ownership, not address magnitude.
//! [`try_read_tracked_gc_header`](crate::value::addr_class::try_read_tracked_gc_header)
//! requires arena page membership or an exact malloc-registry hit, plus a valid
//! `obj_type`/`size`/arena-flag triple, before the first header byte is
//! touched. So this path does not reclassify
//!
//! * genuine subnormal numbers (including pointer-magnitude bit patterns),
//! * unrelated non-Perry allocations, even ones carrying synthetic GC headers,
//! * headerless registry handles and every other handle-band id, or
//! * any NaN-boxed value at all — one `bits >> 48` compare returns those
//!   untouched, so the ordinary receiver pays nothing.
//!
//! ## 4. An unvouched bare word was still read as a pointer, and faulted
//!
//! The mirror image of the above, and the reason this module owns the decision
//! rather than reboxing opportunistically. A genuine positive subnormal double
//! has bits that *look* like an address: `1e-310` is `0x1268_8b70_e62b`, which
//! is above the handle band and inside `is_valid_obj_ptr`'s platform window. The
//! tower's probes are magnitude-gated — `try_read_gc_header` classifies by
//! address range and then dereferences `addr - GC_HEADER_SIZE` — and that
//! contract is written for a STALE heap address, where the page is still
//! mapped. An arbitrary number is not a stale address; nothing was ever mapped
//! there. `(1e-310 as any).toString()` therefore SIGSEGV'd inside
//! `url::search_params::shape_is_url_search_params`, a probe that is itself
//! careful (it gates on `try_read_gc_header` precisely so a Date cell cannot
//! fault it) and simply cannot tell a number from an address. Node prints
//! `1e-310`. A smaller subnormal aliases the *handle* band instead — `5e-324`
//! is `0x1`, so the handle dispatcher answered it and
//! `(5e-324 as any).toString()` returned `undefined` where node returns
//! `"5e-324"`.
//!
//! Fixing the one probe that happened to fault would be whack-a-mole: every
//! magnitude-gated probe in the tower has the same exposure, and this codebase
//! has paid for that pattern three times over (CLAUDE.md, "Known-weak areas").
//! The chokepoint is here. Once an owner has been asked and declined, the word
//! is *definitively not a managed pointer*, so it is the number its bits spell
//! and [`dispatch_unvouched_bare_as_number`] dispatches it as one — it never
//! reaches a pointer-shaped probe at all. That makes the whole class
//! unreachable rather than fixing its current instance.
//!
//! Because of that, the tower's magnitude-only tail recovery is now
//! unreachable for bare words and is deleted with this change: it was the last
//! place that treated address magnitude as proof.

/// Rebox an allocator-proven bare managed receiver into a properly tagged
/// NaN-box, following forwarding. Returns `object` unchanged for everything
/// else. See the module docs for why this must happen before the root.
#[inline]
pub(super) unsafe fn canonicalize_bare_gc_receiver(object: f64) -> f64 {
    let bits = object.to_bits();
    // Every NaN-boxed value — `INT32_TAG` and `SHORT_STRING_TAG` included — has
    // non-zero top 16 bits, and 0 is not an address. One compare returns every
    // ordinary receiver before any probe runs.
    if bits == 0 || (bits >> 48) != 0 {
        return object;
    }
    let addr = bits as usize;
    if !bare_word_has_an_owner(addr) {
        return object;
    }
    if crate::value::addr_class::try_read_tracked_gc_header(addr).is_none() {
        // Vouched for by a header-free registry rather than by the allocator:
        // there is no `GcHeader` to read a kind out of, and every such
        // allocation (a `Symbol.for` symbol, a registered buffer, a typed
        // array) is boxed as a POINTER.
        return f64::from_bits(
            crate::value::POINTER_TAG | (addr as u64 & crate::value::POINTER_MASK),
        );
    }
    // A collection may already have moved it; the root taken by the caller is
    // only useful once it holds the CURRENT address.
    let resolved = crate::value::resolve_forwarding(addr);
    let Some(header) = crate::value::addr_class::try_read_tracked_gc_header(resolved) else {
        // A forwarding target that is no longer a tracked allocation is not
        // something to dispatch on. Leave the value alone rather than
        // manufacture a pointer to it.
        return object;
    };
    let tag = match (*header.as_ptr()).obj_type {
        // `alloc_symbol` gc_mallocs a `SymbolHeader` as `GC_TYPE_STRING`, so the
        // header alone cannot separate a string from a fresh `Symbol()`. Screen
        // on the object's own first word the way
        // `gc_pointer_and_type_from_value` does — `may_be_symbol_header` is
        // exact in the `false` direction, so a string pays one content load and
        // a symbol keeps `POINTER_TAG`, which is how symbols are boxed.
        crate::gc::GC_TYPE_STRING
            if !(crate::symbol::may_be_symbol_header(resolved as *const u8)
                && crate::symbol::is_registered_symbol(resolved)) =>
        {
            crate::value::STRING_TAG
        }
        crate::gc::GC_TYPE_BIGINT => crate::value::BIGINT_TAG,
        _ => crate::value::POINTER_TAG,
    };
    f64::from_bits(tag | (resolved as u64 & crate::value::POINTER_MASK))
}

/// Can any owner answer for `addr` **without dereferencing it**?
///
/// The allocator is the strongest answer (`try_read_tracked_gc_header` proves
/// arena page membership or an exact malloc-registry hit). The rest are the
/// address-keyed registries that own allocations carrying no `GcHeader` at all —
/// a `Symbol.for` symbol, an `ArrayBuffer`/`Uint8Array` backing store, a typed
/// array. Every one of these is a table lookup behind an idle latch, so a
/// program that never made one pays a single atomic load; and every one of them
/// is consulted by `gc_pointer_and_type_from_value` for the same reason.
///
/// Nothing here reads memory at `addr`, which is the whole point: this runs on
/// words that may not be addresses.
#[inline]
fn bare_word_has_an_owner(addr: usize) -> bool {
    let allocator_owns = unsafe { crate::value::addr_class::try_read_tracked_gc_header(addr) };
    allocator_owns.is_some()
        || crate::symbol::is_registered_symbol(addr)
        || crate::buffer::is_registered_buffer(addr)
        || crate::buffer::is_any_array_buffer(addr)
        || crate::buffer::is_uint8array_buffer(addr)
        || crate::typedarray::lookup_typed_array_kind(addr).is_some()
}

/// True when `object` is still a bare word after
/// [`canonicalize_bare_gc_receiver`] has run — i.e. no owner claimed it, so it
/// cannot be a managed pointer.
///
/// `+0.0` (all-zero bits) is deliberately excluded: it is not address-shaped,
/// no probe can mistake it for one, and leaving it on the ordinary path keeps
/// this change to the words that actually alias addresses.
#[inline]
pub(super) fn is_unvouched_bare_word(object: f64) -> bool {
    let bits = object.to_bits();
    bits != 0 && (bits >> 48) == 0
}

/// Dispatch an unvouched bare word as the number its bits spell.
///
/// This is what the tower's `primitive_kind` arm would have done for it, minus
/// the ~1200 lines of pointer-shaped probes in between — any one of which may
/// dereference the word on nothing more than its magnitude. Reaching the same
/// answer without them is the fix for defect 4 above.
#[inline]
pub(super) unsafe fn dispatch_unvouched_bare_as_number(
    object: f64,
    method_name_ptr: *const i8,
    method_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let method_name_cow = if method_name_ptr.is_null() || method_name_len == 0 {
        std::borrow::Cow::Borrowed("")
    } else {
        let bytes = std::slice::from_raw_parts(method_name_ptr as *const u8, method_name_len);
        String::from_utf8_lossy(bytes)
    };
    let method_name: &str = &method_name_cow;
    if let Some(result) = super::call_primitive_builtin_prototype_method(
        object,
        b"Number",
        method_name,
        args_ptr,
        args_len,
    ) {
        return result;
    }
    crate::error::js_throw_type_error_not_a_function(
        b"number".as_ptr(),
        b"number".len(),
        method_name.as_ptr(),
        method_name.len(),
    )
}

#[cfg(test)]
mod tests;
