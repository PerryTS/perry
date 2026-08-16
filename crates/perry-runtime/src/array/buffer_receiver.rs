//! #8137: resolve an `Array.prototype` iteration receiver that is really a
//! Buffer-backed `Uint8Array`, and run the method on it in place.
//!
//! Perry's `new Uint8Array([…])` is a `BufferHeader` (`buffer::js_uint8array_new`),
//! not a `TypedArrayHeader`. It is therefore absent from the typed-array
//! registry, and the `lookup_typed_array_kind` re-dispatch every fused
//! `js_array_*` callback helper performs never answers for it. The helper then
//! reads the `BufferHeader` as an `ArrayHeader`. Both start with
//! `{length: u32, capacity: u32}`, so `length` is CORRECT while the elements —
//! read as NaN-boxed f64 slots at `base + 8 + i*8` over a payload that is one
//! byte per element — are raw bytes reinterpreted, and the read runs
//! `length * 7` bytes past the real payload.
//!
//! That is why the symptom is *garbage values*, not an empty result or a
//! throw, and why any probe that only asks "did we return `[]`?" is blind to
//! it. It is also why a predicate test is vacuous: `u.every(x => x > 0)`
//! answers `true` under node AND under the bug, because `1.29e-318 > 0`. Every
//! test in this family must assert value identity.
//!
//! The answer is the shared uint8 `%TypedArray%.prototype` dispatcher that the
//! *statically* typed receiver already reaches (`dispatch_buffer_method`'s
//! catch-all → `dispatch_uint8_buffer_method`). It reads elements through
//! `js_buffer_get` and passes the ORIGINAL Buffer as the callback's 3rd/4th
//! argument, so — unlike `buffer_receiver_as_uint8_typed_array`, which hands
//! back a COPY and is scoped to the immutable methods — a write through
//! `arr` in `u.forEach((v, i, arr) => { arr[0] = 9 })` still lands on `u`.

use super::ArrayHeader;
use crate::closure::ClosureHeader;

/// NaN-box a callback closure as the `args[0]` the uint8 dispatcher validates.
#[inline]
pub(crate) fn callback_arg(callback: *const ClosureHeader) -> f64 {
    f64::from_bits(crate::value::JSValue::pointer(callback as *const u8).bits())
}

/// Run `method` on `arr` through the uint8 `%TypedArray%.prototype` dispatcher
/// when `arr` is a Buffer-backed `Uint8Array`.
///
/// `Some(result)` — the receiver was resolved and the method ran; the caller
/// must return, converting the NaN-boxed result to its own return type.
/// `None` — not our receiver (or the dispatcher does not implement `method`);
/// the caller keeps its ordinary array path.
///
/// **Call this ABOVE the array-only funnel.** `normalize_array_receiver` is
/// permissive for a registered Buffer (it returns the raw address rather than
/// null), so a re-dispatch below it does still run today — but that is a
/// property of one funnel, and the sibling funnel `clean_arr_ptr` returns NULL
/// for the same receiver, which every caller reads as "empty". Asking the
/// receiver-kind question first is what #8090 / #8119 / #8130 / #8140 each had
/// to restore after it had been placed below one; keeping this call above the
/// funnel means the ordering cannot rot back.
pub(crate) fn buffer_receiver_dispatch(
    arr: *const ArrayHeader,
    method: &str,
    args: &[f64],
) -> Option<f64> {
    let addr = crate::array::array_receiver_addr(arr as *mut ArrayHeader);
    if addr == 0 {
        return None;
    }
    // Cheap negative for the hot path: a receiver that is PROVABLY an
    // arena-backed `GC_TYPE_ARRAY` cannot be a registered Buffer, so an
    // ordinary `[1,2,3].map(…)` reaches NEITHER registry.
    //
    // `arena_payload_has_gc_type` rather than a bare header-byte read (or an
    // open-coded address floor): a Buffer comes in BOTH backings, and an
    // EXTERNAL one — `EXTERNAL_BUFFER_REGISTRY`, `shared_sab::alloc_shared_sab`
    // — has no `GcHeader` at all. The eight bytes below its payload are
    // allocator bookkeeping and can read as any `obj_type`, `GC_TYPE_ARRAY`
    // included. A bare tag read would therefore skip the probe for exactly the
    // receiver this function exists to catch, silently and only sometimes.
    // The predicate range-checks, rejects `HeapSpace::Unknown` for the HEADER
    // address, and validates through `gc_type_info` before trusting the byte;
    // it answers `false` for an external buffer, which falls through to the
    // registry — the authoritative answer. See `array/header.rs`'s
    // `array_receiver_gc_tag` doc (#8142).
    if unsafe { crate::typedarray::arena_payload_has_gc_type(addr, crate::gc::GC_TYPE_ARRAY) } {
        return None;
    }
    // `is_typed_array_buffer` is the same gate `dispatch_buffer_method`'s
    // catch-all uses to reach this dispatcher, so the two receiver populations
    // cannot drift apart. It declines `ArrayBuffer` / `SharedArrayBuffer` /
    // `DataView` (none has `%TypedArray%.prototype`, so node throws rather
    // than answering elements) and the KeyObject / CryptoKey buffers.
    if !crate::object::typed_array_proto_thunks::is_typed_array_buffer(addr) {
        return None;
    }
    // Root the receiver and the callback across the dispatch. Both arrive as
    // raw parameters of a `#[no_mangle]` helper, and a callback allocated by a
    // FRAMELESS caller — the arrow in `holder.u.map(x => x * 2)` — is reachable
    // ONLY through that parameter plus the native stack, which an evacuating
    // minor does not scan. Closures are non-movable, so an unrooted one is
    // swept in place mid-loop and the next dispatch calls freed memory. This is
    // #6081 / gh #6206 exactly; `js_array_map` roots its callback for the same
    // reason, and routing through this function must not lose that root.
    let scope = crate::gc::RuntimeHandleScope::new();
    let _recv = scope.root_nanbox_f64(f64::from_bits(
        crate::value::JSValue::pointer(addr as *const u8).bits(),
    ));
    let _cb = args
        .first()
        .map(|callback| scope.root_nanbox_f64(*callback));
    unsafe {
        crate::object::typed_array_proto_thunks::dispatch_uint8_buffer_method(addr, method, args)
    }
}

/// The `*mut ArrayHeader` a `map`/`filter` caller returns, from the NaN-boxed
/// pointer the dispatcher answers. The result is a `BufferHeader`, matching
/// node (`u8.map(…)` is a `Uint8Array`, not a plain Array) and matching what
/// the typed-array arm beside it already does with a `TypedArrayHeader`.
#[inline]
pub(crate) fn dispatch_result_as_array(result: f64) -> *mut ArrayHeader {
    crate::value::js_nanbox_get_pointer(result) as *mut ArrayHeader
}
