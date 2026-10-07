//! The C ABI of native-payload streams (#11919, STREAM-PAYLOAD-DESIGN), for a
//! stream family that lives in a separately linked binding (`perry-ext-zlib`).
//!
//! perry-ffi `native_stream` mirrors [`StreamHooks`], [`StepIn`], [`StepOut`]
//! and [`PayloadVTable`] with `#[repr(C)]` copies; [`js_perry_stream_abi_layout`]
//! is the digest both sides compute, so a binding written against another
//! layout is refused rather than misread. The family's cell names its static
//! `PayloadVTable` (the payload C ABI of #11919 P0 carries it in the family
//! descriptor); every entry point here forwards to the same runtime code the
//! in-tree families use: one stream state machine.

use crate::native_payload::PayloadVTable;
use crate::node_stream::native_hooks::{StepIn, StepOut, StreamHooks};

/// Revision of this ABI. Bumped with any signature or layout change.
pub const PERRY_STREAM_ABI_VERSION: u8 = 2;

pub(crate) const fn stream_abi_layout() -> u64 {
    use std::mem::{offset_of, size_of};
    (size_of::<StreamHooks>() as u64) << 56
        | (offset_of!(StreamHooks, step) as u64) << 48
        | (size_of::<StepIn>() as u64) << 40
        | (size_of::<StepOut>() as u64) << 32
        | (offset_of!(StepOut, status) as u64) << 24
        | (offset_of!(StepOut, external_bytes) as u64) << 16
        | (size_of::<PayloadVTable>() as u64) << 8
        | PERRY_STREAM_ABI_VERSION as u64
}

/// The stream ABI's layout digest (perry-ffi `native_stream::layout_digest`).
#[no_mangle]
pub extern "C" fn js_perry_stream_abi_layout() -> u64 {
    stream_abi_layout()
}

fn is_object(value: f64) -> bool {
    let jsval = crate::value::JSValue::from_bits(value.to_bits());
    if !jsval.is_pointer() {
        return false;
    }
    let addr = jsval.as_pointer::<u8>() as usize;
    // SAFETY: the reader validates the address before reading.
    unsafe { crate::value::addr_class::try_read_gc_header(addr) }
        .is_some_and(|header| header.obj_type == crate::gc::GC_TYPE_OBJECT)
}

/// node's `Transform` constructor body on a family-allocated object. 0 on
/// success, -1 for a non-object.
#[no_mangle]
pub extern "C" fn js_perry_stream_init_transform(value: f64, opts: f64) -> i32 {
    if !is_object(value) {
        return -1;
    }
    crate::node_stream::init_transform_in_place(value, opts);
    0
}

/// node's `Writable` constructor body on a family-allocated object.
#[no_mangle]
pub extern "C" fn js_perry_stream_init_writable(value: f64, opts: f64) -> i32 {
    if !is_object(value) {
        return -1;
    }
    crate::node_stream::init_writable_payload_in_place(value, opts);
    0
}

/// `.flush(kind, cb)` on a native stream: a `FLUSH` record behind the
/// buffered writes. -1 when `value` is not a native stream.
#[no_mangle]
pub extern "C" fn js_perry_stream_flush(value: f64, kind: i32, callback: f64) -> i32 {
    if crate::node_stream::native_hooks::hooks_of(value).is_none() {
        return -1;
    }
    crate::node_stream::native_hooks::request_flush(value, kind, callback);
    0
}

/// Run a JS function value from the immediate queue (a one-shot codec job
/// whose inputs are its traced captures). -1 when `closure` is not callable.
#[no_mangle]
pub extern "C" fn js_perry_stream_queue_immediate(closure: f64) -> i32 {
    let jsval = crate::value::JSValue::from_bits(closure.to_bits());
    if !jsval.is_pointer() {
        return -1;
    }
    let addr = jsval.as_pointer::<u8>() as usize;
    if !crate::closure::is_closure_ptr(addr) {
        return -1;
    }
    crate::timer::js_set_immediate_callback(addr as i64);
    0
}

/// Base prototype _transform/_flush, sharing the runner and its traced record.
#[no_mangle]
pub extern "C" fn js_perry_stream_prototype_step(
    value: f64,
    chunk: f64,
    callback: f64,
    final_op: i32,
) -> i32 {
    if crate::node_stream::native_hooks::begin_prototype_step(value, chunk, callback, final_op != 0)
    {
        0
    } else {
        -1
    }
}

#[cfg(feature = "keepalive-anchors")]
mod keepalive {
    use super::*;
    #[used(compiler)]
    static DIGEST: extern "C" fn() -> u64 = js_perry_stream_abi_layout;
    #[used(compiler)]
    static TRANSFORM: extern "C" fn(f64, f64) -> i32 = js_perry_stream_init_transform;
    #[used(compiler)]
    static WRITABLE: extern "C" fn(f64, f64) -> i32 = js_perry_stream_init_writable;
    #[used(compiler)]
    static FLUSH: extern "C" fn(f64, i32, f64) -> i32 = js_perry_stream_flush;
    #[used(compiler)]
    static IMMEDIATE: extern "C" fn(f64) -> i32 = js_perry_stream_queue_immediate;
    #[used(compiler)]
    static PROTOTYPE_STEP: extern "C" fn(f64, f64, f64, i32) -> i32 =
        js_perry_stream_prototype_step;
}
