//! Byte-view receivers: a Buffer or `Uint8Array` stored as a `BufferHeader`.
//!
//! Such a cell keeps every own property, accessor and recorded
//! `[[Prototype]]` in its one property bag (`buffer::store::bag`), so a cell
//! with no bag holds none of them: a method it is called with is read from
//! its prototype chain, which the builtin dispatcher answers while
//! `Buffer.prototype` is unwritten (`buffer_dispatch::dispatch_buffer_method`
//! takes the same bit).

use super::*;

/// A Buffer or `Uint8Array` byte cell (an owner or a view over one) with no
/// property bag, while `Buffer.prototype` is unwritten.
#[inline(always)]
pub(in crate::object) unsafe fn byte_view_ok(v: f64) -> bool {
    heap_cell(v).is_some_and(|h| {
        is_byte_view_receiver_type((*h).obj_type)
            && crate::buffer::store::bag(raw(v) as usize).is_null()
    }) && super::super::builtin_proto::prototype_unwritten(BuiltinProto::Buffer)
}

/// The header types [`byte_view_ok`] reads as a Buffer or `Uint8Array`.
#[inline(always)]
pub(in crate::object) fn is_byte_view_receiver_type(obj_type: u8) -> bool {
    matches!(
        obj_type & !crate::codegen_abi::BYTES_TYPE_VIEW,
        crate::gc::GC_TYPE_BUFFER_UINT8ARRAY | crate::gc::GC_TYPE_BUFFER
    )
}

/// `recv.copyWithin(a0)` (families B).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_copyWithin1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if byte_view_ok(recv) {
        return super::super::buffer_dispatch::byte_view_copy_within(recv, a0, 0.0, UNDEFINED);
    }
    let args: [f64; 1] = [a0];
    super::super::method_call::js_method_call_slow(
        site,
        recv,
        key,
        method_id,
        args.as_ptr(),
        args.len(),
    )
}

/// `recv.copyWithin(a0, a1)` (families B).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_copyWithin2(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
    a1: f64,
) -> f64 {
    if byte_view_ok(recv) {
        return super::super::buffer_dispatch::byte_view_copy_within(recv, a0, a1, UNDEFINED);
    }
    let args: [f64; 2] = [a0, a1];
    super::super::method_call::js_method_call_slow(
        site,
        recv,
        key,
        method_id,
        args.as_ptr(),
        args.len(),
    )
}

/// `recv.copyWithin(a0, a1, a2)` (families B).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_copyWithin3(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
    a1: f64,
    a2: f64,
) -> f64 {
    if byte_view_ok(recv) {
        return super::super::buffer_dispatch::byte_view_copy_within(recv, a0, a1, a2);
    }
    let args: [f64; 3] = [a0, a1, a2];
    super::super::method_call::js_method_call_slow(
        site,
        recv,
        key,
        method_id,
        args.as_ptr(),
        args.len(),
    )
}

#[cfg(test)]
#[path = "byte_view_tests.rs"]
mod tests;
