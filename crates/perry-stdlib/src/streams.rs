//! Web Streams API (issue #237).
//!
//! Implements `ReadableStream` / `WritableStream` / `TransformStream` plus
//! the matching default reader / writer pair, and the per-controller
//! enqueue / close / error / write / abort surface. Wires `blob.stream()`
//! and `response.body` so the consumers in the issue's acceptance
//! criteria all work end-to-end.
//!
//! Handles use the same numeric f64 ABI as `BLOB_REGISTRY` /
//! `FETCH_RESPONSES` (registry id cast to f64). Codegen's `module ==
//! "readable_stream"` / `"reader"` / `"writable_stream"` / `"writer"` /
//! `"transform_stream"` arms in `lower_call.rs` route methods through
//! these FFIs.
//!
//! Native fetch producers enqueue incrementally with transport byte credit.
//! Buffered Blob sources still deliver their stored payload.
//!
//! BYOB readers (`getReader({ mode: "byob" })`, `read(view)`,
//! `controller.byobRequest.respond/respondWithNewView`) and real
//! `QueuingStrategy` size accounting (per-chunk `size()` results summed
//! into `desiredSize`) live in `streams/byob.rs` and the queue helpers on
//! `ReadableStreamData` (#4915).

mod errors;
mod statics;
use errors::{stream_range_error_with_code, stream_type_error, stream_type_error_with_code};
use perry_runtime::closure::{JsFunctionInfo, JsThis};
use perry_runtime::{ArrayHeader, ClosureHeader, JSValue, ObjectHeader, Promise, StringHeader};
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
// Calls that allocate or mutate runtime-owned values must cross the stable C ABI. A shared stdlib still contains fallback Rust runtime glue for generic
// monomorphizations; direct Rust calls would allocate into that image's arena
// instead of the process-wide runtime provider.
// `Promise` crosses this boundary by pointer only: both sides are this workspace's
// Rust and never read its fields through the declaration.
#[allow(improper_ctypes)]
extern "C" {
    #[link_name = "js_array_alloc"]
    fn provider_js_array_alloc(capacity: u32) -> *mut ArrayHeader;
    #[link_name = "js_array_push"]
    fn provider_js_array_push(array: *mut ArrayHeader, value: JSValue) -> *mut ArrayHeader;
    #[link_name = "js_array_get"]
    fn provider_js_array_get(array: *const ArrayHeader, index: u32) -> JSValue;
    #[link_name = "js_array_is_array"]
    fn provider_js_array_is_array(value: f64) -> f64;
    #[link_name = "js_array_length"]
    fn provider_js_array_length(array: *const ArrayHeader) -> u32;
    #[link_name = "js_array_push_f64"]
    fn provider_js_array_push_f64(array: *mut ArrayHeader, value: f64) -> *mut ArrayHeader;
    #[link_name = "js_iterator_to_array"]
    fn provider_js_iterator_to_array(iterator: f64) -> *mut ArrayHeader;
    #[link_name = "js_assimilate_thenable"]
    fn provider_js_assimilate_thenable(value: f64) -> f64;
    #[link_name = "js_closure_alloc"]
    fn provider_js_closure_alloc(
        info: *const JsFunctionInfo,
        capture_count: u32,
    ) -> *mut ClosureHeader;
    #[link_name = "js_closure_call0"]
    fn provider_js_closure_call0(closure: *const ClosureHeader, this: JsThis) -> f64;
    #[link_name = "js_closure_call1"]
    fn provider_js_closure_call1(closure: *const ClosureHeader, this: JsThis, arg0: f64) -> f64;
    #[link_name = "js_closure_call2"]
    fn provider_js_closure_call2(
        closure: *const ClosureHeader,
        this: JsThis,
        arg0: f64,
        arg1: f64,
    ) -> f64;
    #[link_name = "js_closure_get_capture_ptr"]
    fn provider_js_closure_get_capture_ptr(closure: *const ClosureHeader, index: u32) -> i64;
    #[link_name = "js_closure_set_capture_ptr"]
    fn provider_js_closure_set_capture_ptr(closure: *mut ClosureHeader, index: u32, value: i64);
    #[link_name = "js_nanbox_get_pointer"]
    fn provider_js_nanbox_get_pointer(value: f64) -> i64;
    #[link_name = "js_object_alloc"]
    fn provider_js_object_alloc(class_id: u32, field_count: u32) -> *mut ObjectHeader;
    #[link_name = "js_object_get_field_by_name"]
    fn provider_js_object_get_field_by_name(
        object: *const ObjectHeader,
        key: *const StringHeader,
    ) -> JSValue;
    #[link_name = "js_object_set_field"]
    fn provider_js_object_set_field(object: *mut ObjectHeader, index: u32, value: JSValue);
    #[link_name = "js_object_set_field_by_name"]
    fn provider_js_object_set_field_by_name(
        object: *mut ObjectHeader,
        key: *const StringHeader,
        value: f64,
    );
    #[link_name = "js_object_set_keys"]
    fn provider_js_object_set_keys(object: *mut ObjectHeader, keys: *mut ArrayHeader);
    #[link_name = "js_promise_mark_internally_handled"]
    fn provider_js_promise_mark_internally_handled(promise: *mut Promise);
    #[link_name = "js_native_call_value"]
    fn provider_js_native_call_value(
        function: f64,
        this: JsThis,
        arguments: *const f64,
        argument_count: usize,
    ) -> f64;
    #[link_name = "js_ffi_root_scope_enter"]
    fn provider_js_ffi_root_scope_enter() -> usize;
    #[link_name = "js_ffi_root_push_nanbox"]
    fn provider_js_ffi_root_push_nanbox(bits: u64) -> usize;
    #[link_name = "js_ffi_root_get_nanbox"]
    fn provider_js_ffi_root_get_nanbox(index: usize) -> u64;
    #[link_name = "js_ffi_root_scope_exit"]
    fn provider_js_ffi_root_scope_exit(base: usize);
    #[link_name = "js_promise_new"]
    fn provider_js_promise_new() -> *mut Promise;
    #[link_name = "js_promise_all"]
    fn provider_js_promise_all(promises: *const ArrayHeader) -> *mut Promise;
    #[link_name = "js_promise_reason"]
    fn provider_js_promise_reason(promise: *mut Promise) -> f64;
    #[link_name = "js_promise_reject"]
    fn provider_js_promise_reject(promise: *mut Promise, reason: f64);
    #[link_name = "js_promise_resolve"]
    fn provider_js_promise_resolve(promise: *mut Promise, value: f64);
    #[link_name = "js_promise_resolve_with_promise"]
    fn provider_js_promise_resolve_with_promise(outer: *mut Promise, inner: *mut Promise);
    #[link_name = "js_promise_run_microtasks"]
    fn provider_js_promise_run_microtasks() -> i32;
    #[link_name = "js_promise_state"]
    fn provider_js_promise_state(promise: *mut Promise) -> i32;
    #[link_name = "js_promise_then"]
    fn provider_js_promise_then(
        promise: *mut Promise,
        on_fulfilled: *const ClosureHeader,
        on_rejected: *const ClosureHeader,
    ) -> *mut Promise;
    #[link_name = "js_promise_value"]
    fn provider_js_promise_value(promise: *mut Promise) -> f64;
    #[link_name = "js_string_from_bytes"]
    fn provider_js_string_from_bytes(data: *const u8, len: u32) -> *mut StringHeader;
    #[link_name = "js_value_is_promise"]
    fn provider_js_value_is_promise(value: f64) -> i32;
}

macro_rules! provider_call {
    ($name:ident ( $($argument:expr),* $(,)? )) => {
        unsafe { $name($($argument),*) }
    };
}

fn js_array_alloc(capacity: u32) -> *mut ArrayHeader {
    provider_call!(provider_js_array_alloc(capacity))
}

fn js_array_push(array: *mut ArrayHeader, value: JSValue) -> *mut ArrayHeader {
    provider_call!(provider_js_array_push(array, value))
}

fn js_array_get(array: *const ArrayHeader, index: u32) -> JSValue {
    provider_call!(provider_js_array_get(array, index))
}

fn js_array_is_array(value: f64) -> f64 {
    provider_call!(provider_js_array_is_array(value))
}

fn js_array_length(array: *const ArrayHeader) -> u32 {
    provider_call!(provider_js_array_length(array))
}

fn js_array_push_f64(array: *mut ArrayHeader, value: f64) -> *mut ArrayHeader {
    provider_call!(provider_js_array_push_f64(array, value))
}

fn js_iterator_to_array(iterator: f64) -> *mut ArrayHeader {
    provider_call!(provider_js_iterator_to_array(iterator))
}

fn js_assimilate_thenable(value: f64) -> f64 {
    provider_call!(provider_js_assimilate_thenable(value))
}

fn js_closure_alloc(info: *const JsFunctionInfo, capture_count: u32) -> *mut ClosureHeader {
    provider_call!(provider_js_closure_alloc(info, capture_count))
}

fn js_closure_call0(closure: *const ClosureHeader, this: JsThis) -> f64 {
    provider_call!(provider_js_closure_call0(closure, this))
}

fn js_closure_call1(closure: *const ClosureHeader, this: JsThis, arg0: f64) -> f64 {
    provider_call!(provider_js_closure_call1(closure, this, arg0))
}

fn js_closure_call2(closure: *const ClosureHeader, this: JsThis, arg0: f64, arg1: f64) -> f64 {
    provider_call!(provider_js_closure_call2(closure, this, arg0, arg1))
}

fn js_closure_get_capture_ptr(closure: *const ClosureHeader, index: u32) -> i64 {
    provider_call!(provider_js_closure_get_capture_ptr(closure, index))
}

fn js_closure_set_capture_ptr(closure: *mut ClosureHeader, index: u32, value: i64) {
    provider_call!(provider_js_closure_set_capture_ptr(closure, index, value))
}

fn js_nanbox_get_pointer(value: f64) -> i64 {
    provider_call!(provider_js_nanbox_get_pointer(value))
}

fn js_object_alloc(class_id: u32, field_count: u32) -> *mut ObjectHeader {
    provider_call!(provider_js_object_alloc(class_id, field_count))
}

fn js_object_get_field_by_name(object: *const ObjectHeader, key: *const StringHeader) -> JSValue {
    provider_call!(provider_js_object_get_field_by_name(object, key))
}

fn js_object_set_field(object: *mut ObjectHeader, index: u32, value: JSValue) {
    provider_call!(provider_js_object_set_field(object, index, value))
}

fn js_object_set_field_by_name(object: *mut ObjectHeader, key: *const StringHeader, value: f64) {
    provider_call!(provider_js_object_set_field_by_name(object, key, value))
}

fn js_object_set_keys(object: *mut ObjectHeader, keys: *mut ArrayHeader) {
    provider_call!(provider_js_object_set_keys(object, keys))
}

fn js_promise_mark_internally_handled(promise: *mut Promise) {
    provider_call!(provider_js_promise_mark_internally_handled(promise))
}

/// Call `function` with `this` bound to `receiver` (a method call of a
/// function value, `function.call(receiver)`).
fn js_native_call_value(
    function: f64,
    receiver: f64,
    arguments: *const f64,
    argument_count: usize,
) -> f64 {
    provider_call!(provider_js_native_call_value(
        function,
        perry_runtime::closure::JsThis::from_f64(receiver),
        arguments,
        argument_count
    ))
}

fn js_promise_new() -> *mut Promise {
    provider_call!(provider_js_promise_new())
}

fn js_promise_all(promises: *const ArrayHeader) -> *mut Promise {
    provider_call!(provider_js_promise_all(promises))
}

fn js_promise_reason(promise: *mut Promise) -> f64 {
    provider_call!(provider_js_promise_reason(promise))
}

fn js_promise_reject(promise: *mut Promise, reason: f64) {
    provider_call!(provider_js_promise_reject(promise, reason))
}

fn js_promise_resolve(promise: *mut Promise, value: f64) {
    provider_call!(provider_js_promise_resolve(promise, value))
}

fn js_promise_resolve_with_promise(outer: *mut Promise, inner: *mut Promise) {
    provider_call!(provider_js_promise_resolve_with_promise(outer, inner))
}

fn js_promise_run_microtasks() -> i32 {
    provider_call!(provider_js_promise_run_microtasks())
}

fn js_promise_state(promise: *mut Promise) -> i32 {
    provider_call!(provider_js_promise_state(promise))
}

fn js_promise_then(
    promise: *mut Promise,
    on_fulfilled: *const ClosureHeader,
    on_rejected: *const ClosureHeader,
) -> *mut Promise {
    provider_call!(provider_js_promise_then(promise, on_fulfilled, on_rejected))
}

fn js_promise_value(promise: *mut Promise) -> f64 {
    provider_call!(provider_js_promise_value(promise))
}

fn js_string_from_bytes(data: *const u8, len: u32) -> *mut StringHeader {
    provider_call!(provider_js_string_from_bytes(data, len))
}

fn js_value_is_promise(value: f64) -> i32 {
    provider_call!(provider_js_value_is_promise(value))
}

/// Allocate a promise the stream machinery owns and observes internally — the
/// reader/writer `closed`, writer `ready`, and `[[closeRequest]]` promises.
/// Node marks these `markPromiseAsHandled` so that an abort / error / cancel
/// that rejects them is never surfaced as an unhandled rejection (#1545).
pub(crate) fn internal_promise() -> *mut Promise {
    let p = js_promise_new();
    js_promise_mark_internally_handled(p);
    p
}

unsafe fn try_call_stream_action(callback: i64, reason: f64) -> Result<f64, u64> {
    perry_runtime::exception::catch_js_throw(|| {
        js_closure_call1(
            callback as *const ClosureHeader,
            perry_runtime::closure::plain_call_receiver(),
            reason,
        )
    })
    .map_err(f64::to_bits)
}

unsafe fn stream_action_promise(result: f64) -> Option<*mut Promise> {
    let adopted = js_assimilate_thenable(result);
    if js_value_is_promise(adopted) == 0 {
        return None;
    }
    let promise = js_nanbox_get_pointer(adopted) as *mut Promise;
    (!promise.is_null()).then_some(promise)
}

unsafe fn settle_stream_action_promise(promise: *mut Promise, actions: &[*mut Promise]) {
    match actions {
        [] => js_promise_resolve(promise, f64::from_bits(TAG_UNDEFINED)),
        [action] => js_promise_resolve_with_promise(promise, *action),
        _ => {
            let values = js_array_alloc(actions.len() as u32);
            for action in actions {
                js_array_push(values, JSValue::pointer(*action as *const u8));
            }
            let all = js_promise_all(values);
            js_promise_resolve_with_promise(promise, all);
        }
    }
}

mod byob;
mod state;
#[cfg(test)]
pub(crate) use state::stream_registry_presence_for_test;
use state::*;
pub(crate) use state::{ReaderRef, RsRef, TsRef, WriterRef, WsRef};
mod expando;
mod native_properties;
pub(crate) use native_properties::{native_property_bag, set_native_property_bag};
mod controller;
mod from_source;
mod gc;
mod idalloc;
mod native;
mod pipe;
mod pull;
use pull::{close_pending, error_pending, invoke_start};
pub(super) use pull::{maybe_pull, maybe_pull_force};
mod readable;
#[cfg(test)]
use controller::readable_controller_enqueue_error;
use controller::{
    error_readable_stream, invalid_size_message, readable_strategy_size_to_number,
    throw_invalid_readable_strategy_size,
};
pub use controller::{
    js_readable_stream_controller_close, js_readable_stream_controller_desired_size,
    js_readable_stream_controller_enqueue, js_readable_stream_controller_error,
};
#[cfg(test)]
use from_source::call_iterator_next;
pub(crate) use from_source::has_async_iterator;
pub use from_source::{
    js_jsx_render_stream_from_value, js_readable_stream_from_blob,
    js_readable_stream_from_iterable, js_readable_stream_from_response,
};
pub(crate) use readable::alloc_readable_from_bytes;
pub(super) use readable::{acquire_reader, AcquireError};
pub use readable::{
    js_readable_stream_cancel, js_readable_stream_deferred_byte_source,
    js_readable_stream_get_reader, js_readable_stream_get_reader_with_options,
    js_readable_stream_locked, js_readable_stream_new, js_readable_stream_new_from_source_object,
    js_readable_stream_new_with_source_type, js_readable_stream_new_with_strategy_and_source_type,
};
use readable::{js_readable_stream_cancel_inner, js_string_value_to_string, value_string_equals};
mod reader;
pub use reader::{
    js_readable_stream_values, js_reader_cancel, js_reader_closed, js_reader_read,
    js_reader_release_lock,
};
mod pipe_through;
pub(crate) use native::{
    alloc_native_readable, collect_body, native_capacity, native_enqueue, take_native_promise,
};
pub use pipe_through::{
    js_readable_stream_pipe_through, js_readable_stream_pipe_through_pair,
    js_readable_stream_pipe_through_validate,
};
#[cfg(test)]
use pipe_through::{pipe_through_signal_error, pipe_through_validation_error};
mod prototypes;
mod strategy;
mod subclass;
mod tee;
#[cfg(test)]
pub(crate) mod tests;
mod transform;
mod writable;

pub use tee::js_readable_stream_tee;
pub(crate) use tee::tee_readable_stream;
use tee::{tee_branches_of, tee_error_branches, tee_source_of};

pub use self::byob::{
    js_readable_stream_controller_byob_request, js_readable_stream_get_byob_reader,
    js_reader_read_with_view,
};
use self::gc::ensure_gc_registered;
#[cfg(test)]
use self::gc::scan_stream_roots;
pub(crate) use self::strategy::parse_strategy_value;
pub use self::strategy::{
    js_byte_length_queuing_strategy_new, js_count_queuing_strategy_new,
    js_streams_strategy_high_water_mark,
};
use self::strategy::{read_high_water_mark, read_queuing_strategy_size};

#[cfg(test)]
pub(crate) use self::byob::byob_pending_for_test;
pub(crate) use self::expando::stream_expando_get;
#[cfg(test)]
pub(crate) use self::expando::stream_expando_set_hook;
use self::pipe::js_readable_stream_pipe_to;
pub(crate) use self::prototypes::install_stream_prototypes;
#[cfg(test)]
pub(crate) use self::state::transform_side_tables_hold_for_test;
use self::subclass::{box_promise, js_stream_unwrap_handle};
pub(crate) use self::subclass::{dispatch_stream_method, dispatch_stream_property};
pub use self::subclass::{
    js_readable_stream_subclass_init, js_stream_handle_is_registered, js_stream_handle_kind,
    js_transform_stream_subclass_init, js_writable_stream_subclass_init,
};
pub use self::transform::{
    js_stream_web_compression_stream_new, js_stream_web_decompression_stream_new,
    js_stream_web_text_decoder_stream_new, js_stream_web_text_encoder_stream_new,
    js_transform_stream_new, js_transform_stream_new_from_transformer_object,
    js_transform_stream_readable, js_transform_stream_writable,
};
use self::transform::{transform_close, transform_writable_for_readable, transform_write};
pub use self::writable::{
    js_writable_stream_abort, js_writable_stream_close, js_writable_stream_get_writer,
    js_writable_stream_locked, js_writable_stream_new, js_writable_stream_new_from_sink_object,
    js_writable_stream_new_with_sink_type, js_writable_stream_throw_invalid_sink, js_writer_abort,
    js_writer_close, js_writer_closed, js_writer_desired_size, js_writer_ready,
    js_writer_release_lock, js_writer_write,
};
use self::writable::{js_writable_stream_abort_inner, writable_stream_write};

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReadableState {
    Readable,
    Closed,
    Errored,
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum WritableState {
    Writable,
    Closing,
    Closed,
    Errored,
}

/// ClearAlgorithms of `stream_id`'s readable controller.
fn readable_clear_algorithms(stream_id: usize) {
    with_rs(RsRef(stream_id), |s| s.clear_algorithms());
}

/// ClearAlgorithms of `stream_id`'s writable controller.
pub(super) fn writable_clear_algorithms(stream_id: usize) {
    with_ws(WsRef(stream_id), |s| s.clear_algorithms());
}

/// TransformStreamDefaultControllerClearAlgorithms for the TransformStream
/// whose writable side is `writable_id` (a no-op for a plain writable): runs
/// with the sink close algorithm (after flush), the sink abort algorithm
/// (abort, readable cancel, terminate) and TransformStreamError.
pub(super) fn transform_clear_algorithms(writable_id: usize) {
    let Some(transform) = transform_of_writable(WsRef(writable_id)) else {
        return;
    };
    with_ts(transform, |t| {
        t.transform_cb = 0;
        t.flush_cb = 0;
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WebCompressionFormat {
    Gzip,
    Deflate,
    DeflateRaw,
    Brotli,
}

enum NativeTransformKind {
    TextEncoder,
    TextDecoder {
        fatal: bool,
        pending: Vec<u8>,
    },
    Compression {
        format: WebCompressionFormat,
        decompress: bool,
        chunks: Vec<u8>,
    },
}

// Band boundaries owned by `perry_runtime::value::addr_class` (the runtime's
// tagged stream-handle probes classify against the same range).
pub(crate) const STREAM_HANDLE_ID_START: usize =
    perry_runtime::value::addr_class::STREAM_ID_BAND_START;
pub(crate) const STREAM_HANDLE_ID_END: usize = perry_runtime::value::addr_class::STREAM_ID_BAND_END;

/// RSTREAM R1: a ReadableStream's controller is its own value, not the stream
/// handle. Its id is the stream id with this bit set, so the controller needs
/// no registry: stream/reader/writer/transform ids are allocated below
/// `STREAM_ALLOC_END`, and `id | STREAM_CONTROLLER_BIT` fills the top half.
pub(crate) const STREAM_CONTROLLER_BIT: usize = (STREAM_HANDLE_ID_END - STREAM_HANDLE_ID_START) / 2;
/// A byte stream's BYOB request is its own value the same way (no object, no
/// registry): `id | STREAM_BYOB_REQUEST_BIT` fills the second quarter.
pub(crate) const STREAM_BYOB_REQUEST_BIT: usize =
    (STREAM_HANDLE_ID_END - STREAM_HANDLE_ID_START) / 4;
/// End of the allocator's range: the first quarter of the stream band.
pub(crate) const STREAM_ALLOC_END: usize = STREAM_HANDLE_ID_START + STREAM_BYOB_REQUEST_BIT;
const _: () = assert!(STREAM_CONTROLLER_BIT.is_power_of_two());
const _: () = assert!(STREAM_BYOB_REQUEST_BIT.is_power_of_two());
const _: () =
    assert!(STREAM_HANDLE_ID_START & (STREAM_CONTROLLER_BIT | STREAM_BYOB_REQUEST_BIT) == 0);
const _: () = assert!(
    (STREAM_ALLOC_END - 1) | STREAM_CONTROLLER_BIT
        == STREAM_HANDLE_ID_END - STREAM_BYOB_REQUEST_BIT - 1
);
const _: () = assert!(
    (STREAM_ALLOC_END - 1) | STREAM_BYOB_REQUEST_BIT
        == STREAM_HANDLE_ID_START + STREAM_CONTROLLER_BIT - 1
);

/// The controller value of readable stream `id`.
#[inline]
pub(crate) fn controller_value(id: usize) -> f64 {
    stream_value(id | STREAM_CONTROLLER_BIT)
}

/// The readable stream id a controller value controls; 0 (a registry miss)
/// for every other value, a stream handle included.
#[inline]
pub(crate) fn controller_stream_id(value: f64) -> usize {
    controller_id_stream(stream_id(value))
}

/// The readable stream id of controller id `id`, else 0.
#[inline]
pub(crate) fn controller_id_stream(id: usize) -> usize {
    if (STREAM_HANDLE_ID_START + STREAM_CONTROLLER_BIT..STREAM_HANDLE_ID_END).contains(&id) {
        id & !STREAM_CONTROLLER_BIT
    } else {
        0
    }
}

/// The BYOB request value of byte stream `id`.
#[inline]
pub(crate) fn byob_request_value(id: usize) -> f64 {
    stream_value(id | STREAM_BYOB_REQUEST_BIT)
}

/// The byte stream id of BYOB request id `id`, else 0.
#[inline]
pub(crate) fn byob_request_id_stream(id: usize) -> usize {
    if (STREAM_ALLOC_END..STREAM_HANDLE_ID_START + STREAM_CONTROLLER_BIT).contains(&id) {
        id & !STREAM_BYOB_REQUEST_BIT
    } else {
        0
    }
}

/// The JS value of a stream-family registry id (stream, reader, writer,
/// transform): a `POINTER_TAG` handle in the stream band, like every other
/// registry handle. Never a number — a number is always a Number, so
/// `typeof 1048576` cannot depend on whether stream 0x100000 is live.
#[inline]
pub(crate) fn stream_value(id: usize) -> f64 {
    perry_runtime::value::js_nanbox_handle(id as i64)
}

/// True for a JS object (a heap `POINTER_TAG` value), false for a stream
/// handle and for every primitive. pipeThrough's pair/options decoding keeps
/// handles on the direct endpoint getters and roots only real objects.
#[inline]
pub(crate) fn is_object_value(value: f64) -> bool {
    JSValue::from_bits(value.to_bits()).is_pointer()
        && perry_runtime::object::stream_handle_id(value).is_none()
}

/// The registry id a JS value names: the payload of a stream-band
/// `POINTER_TAG` handle, else 0 (which no registry holds). Every stream FFI
/// decodes its handle arguments here.
#[inline]
pub(crate) fn stream_id(value: f64) -> usize {
    perry_runtime::object::stream_handle_id(value).unwrap_or(0)
}

/// The error a readable stream ended with, if any. A stream built from an
/// iterable that threw keeps it in `pending_error_after_chunks` until a reader
/// has popped the queued chunks, so check both.
pub(crate) fn readable_stream_error(stream: RsRef) -> Option<f64> {
    with_rs(stream, |s| match s.state {
        ReadableState::Errored => Some(f64::from_bits(s.error_value)),
        _ => s.pending_error_after_chunks.map(f64::from_bits),
    })
    .flatten()
}

// #1545: ONE id allocator shared across all five Web Streams registries.
// Stream handles are `POINTER_TAG` registry handles in their own sub-band of
// the handle band (`stream_value` / `stream_id`), disjoint from the
// Fetch/native/proxy ids, so the runtime's small-handle dispatch routes
// dynamic calls like `src.pipeThrough(ts).getReader()`. #6602: ids of
// terminal objects recycle through `idalloc` so the finite band survives
// long-running servers.
use self::idalloc::next_stream_id;

// ─────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────

unsafe fn closure_from_bits(bits: u64) -> i64 {
    if bits == TAG_UNDEFINED || bits == TAG_NULL || bits == 0 {
        return 0;
    }
    let top = bits >> 48;
    if top >= 0x7FF8 {
        (bits & POINTER_MASK) as i64
    } else {
        0
    }
}

unsafe fn stream_object_field(object: f64, name: &[u8]) -> f64 {
    let value = JSValue::from_bits(object.to_bits());
    if !value.is_pointer() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let obj = js_nanbox_get_pointer(object) as *const ObjectHeader;
    if obj.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    f64::from_bits(js_object_get_field_by_name(obj, key).bits())
}

unsafe fn stream_object_closure(object: f64, name: &[u8]) -> i64 {
    closure_from_bits(stream_object_field(object, name).to_bits())
}

unsafe fn build_iter_result(value_bits: u64, done: bool) -> u64 {
    perry_runtime::iter_result::make_sqlite_iter_result(JSValue::from_bits(value_bits), done)
        .to_bits()
}

pub(crate) unsafe fn alloc_uint8array_from_bytes(bytes: &[u8]) -> u64 {
    use perry_runtime::buffer::bytes::{self, Brand};
    // Placement sees the final brand before applying Buffer pooling policy.
    #[cfg(test)]
    if std::env::var("PERRY_B3_STREAM_SABOTAGE").ok().as_deref() == Some("brand") {
        // Reproduce the old copying Buffer birth followed by rebranding.
        let (value, pin) = bytes::new_bytes(Brand::Buffer, bytes.len(), bytes::Init::PoolCopy);
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), pin.as_mut_ptr(), bytes.len());
        return value.to_bits();
    }
    bytes::from_slice(Brand::Uint8Array, bytes).to_bits()
}

unsafe fn read_bytes_from_chunk(chunk_bits: u64) -> Option<Vec<u8>> {
    let top = chunk_bits >> 48;
    let addr = if top == 0x7FFD || top == 0x7FFF {
        (chunk_bits & POINTER_MASK) as usize
    } else if top == 0 && chunk_bits >= 0x10000 {
        chunk_bits as usize
    } else {
        return None;
    };
    if addr < 0x1000 {
        return None;
    }
    perry_runtime::buffer::bytes::no_gc(|scope| {
        let value = f64::from_bits(JSValue::pointer(addr as *const u8).bits());
        perry_runtime::buffer::bytes::bytes(value, scope)
            .ok()
            .map(<[u8]>::to_vec)
    })
}

unsafe fn raw_pointer_addr(bits: u64) -> Option<usize> {
    let top = bits >> 48;
    if top == 0x7FFD || top == 0x7FFF {
        Some((bits & POINTER_MASK) as usize)
    } else if top == 0 && bits >= 0x10000 {
        Some(bits as usize)
    } else {
        None
    }
}

unsafe fn is_byte_stream_enqueue_chunk(chunk_bits: u64) -> bool {
    let Some(addr) = raw_pointer_addr(chunk_bits) else {
        return false;
    };
    perry_runtime::typedarray_hook_slots::lookup_typed_array_kind(addr).is_some()
        || perry_runtime::buffer::is_data_view(addr)
        || perry_runtime::buffer::is_uint8array_buffer(addr)
        || (perry_runtime::buffer::is_registered_buffer(addr)
            && !perry_runtime::buffer::is_any_array_buffer(addr))
}

unsafe fn make_error_with_message(msg: &str) -> u64 {
    let s = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = perry_runtime::error::js_error_new_with_message(s);
    JSValue::pointer(err as *const u8).bits()
}

unsafe fn make_type_error_with_message(msg: &str) -> u64 {
    stream_type_error(msg).value().to_bits()
}

unsafe fn make_type_error_with_code(message: &str, code: &'static str) -> u64 {
    stream_type_error_with_code(message, code).value().to_bits()
}

unsafe fn throw_type_error(message: &str) -> ! {
    let err = stream_type_error(message);
    perry_runtime::exception::throw_core(err)
}

unsafe fn throw_type_error_with_code(message: &str, code: &'static str) -> ! {
    let err = stream_type_error_with_code(message, code);
    perry_runtime::exception::throw_core(err)
}

unsafe fn throw_range_error_with_code(message: &str, code: &'static str) -> ! {
    let err = stream_range_error_with_code(message, code);
    perry_runtime::exception::throw_core(err)
}

unsafe fn reject_type_error(promise: *mut Promise, message: &str) {
    let err = make_type_error_with_message(message);
    js_promise_reject(promise, f64::from_bits(err));
}

unsafe fn throw_invalid_arg_type(message: &str) -> ! {
    let s = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    perry_runtime::node_submodules::register_error_code_pub(s, "ERR_INVALID_ARG_TYPE");
    let err = perry_runtime::error::CoreError::type_error(s);
    perry_runtime::exception::throw_core(err)
}

fn alloc_readable(start_cb: i64, pull_cb: i64, cancel_cb: i64, hwm: f64) -> usize {
    alloc_readable_with_type(start_cb, pull_cb, cancel_cb, hwm, false)
}

fn alloc_readable_with_type(
    start_cb: i64,
    pull_cb: i64,
    cancel_cb: i64,
    hwm: f64,
    is_byte_stream: bool,
) -> usize {
    alloc_readable_with_strategy(start_cb, pull_cb, cancel_cb, hwm, is_byte_stream, 0)
}

fn alloc_readable_with_strategy(
    start_cb: i64,
    pull_cb: i64,
    cancel_cb: i64,
    hwm: f64,
    is_byte_stream: bool,
    strategy_size_cb: i64,
) -> usize {
    let id = next_stream_id();
    // Byte streams default to highWaterMark 0 (per spec — no eager pull at
    // construction; the first pull fires when a read request arrives, #4915).
    // Default streams keep the legacy clamp-to-1 behavior.
    let high_water_mark = if hwm.is_nan() {
        if is_byte_stream {
            0.0
        } else {
            1.0
        }
    } else if !is_byte_stream && hwm <= 0.0 {
        1.0
    } else {
        hwm.max(0.0)
    };
    insert_rs(
        RsRef(id),
        ReadableStreamData::new(
            start_cb,
            pull_cb,
            cancel_cb,
            strategy_size_cb,
            high_water_mark,
            is_byte_stream,
        ),
    );
    id
}

fn alloc_writable(write_cb: i64, close_cb: i64, abort_cb: i64, hwm: f64) -> usize {
    alloc_writable_with_strategy(write_cb, close_cb, abort_cb, hwm, 0)
}

fn alloc_writable_with_strategy(
    write_cb: i64,
    close_cb: i64,
    abort_cb: i64,
    hwm: f64,
    strategy_size_cb: i64,
) -> usize {
    let id = next_stream_id();
    let ready = internal_promise();
    let closed = internal_promise();
    js_promise_resolve(ready, f64::from_bits(TAG_UNDEFINED));
    let high_water_mark = if hwm.is_nan() || hwm <= 0.0 { 1.0 } else { hwm };
    insert_ws(
        WsRef(id),
        WritableStreamData::new(
            write_cb,
            close_cb,
            abort_cb,
            strategy_size_cb,
            high_water_mark,
            ready,
            closed,
        ),
    );
    id
}

pub(crate) use self::subclass::{
    alloc_read_body_stream, drain_readable_into_bytes, is_readable_stream, readable_body_state,
};

#[cfg(test)]
mod handle_boundary_tests {
    use super::*;

    #[test]
    fn raw_and_tagged_typed_receivers_store_stream_expandos() {
        crate::test_runtime::initialize();
        unsafe {
            let stream = js_readable_stream_new(
                f64::from_bits(TAG_UNDEFINED),
                f64::from_bits(TAG_UNDEFINED),
                f64::from_bits(TAG_UNDEFINED),
                1.0,
            );
            let id = stream_id(stream);
            unsafe extern "C" fn probe(id: usize) -> bool {
                js_stream_handle_kind(id) != 0
            }
            perry_runtime::object::js_register_stream_handle_probe(probe);
            for (receiver, key_name, value) in
                [(id as u64, "raw", 1.0), (stream.to_bits(), "tagged", 2.0)]
            {
                let key =
                    perry_runtime::js_string_from_bytes(key_name.as_ptr(), key_name.len() as u32);
                if receiver == id as u64 {
                    perry_runtime::object::js_object_set_field_by_name(
                        receiver as *mut perry_runtime::object::ObjectHeader,
                        key,
                        value,
                    );
                } else {
                    let object = f64::from_bits(receiver);
                    let property = perry_runtime::value::js_nanbox_string(key as i64);
                    perry_runtime::proxy::js_put_value_set(object, property, value, object, 0);
                }
                assert_eq!(stream_expando_get(id, key_name), Some(value));
            }
        }
    }

    #[test]
    fn blob_and_response_bridge_results_are_tagged() {
        unsafe {
            let blob = js_readable_stream_from_blob(0.0);
            assert!(stream_id(blob) >= STREAM_HANDLE_ID_START);
            #[cfg(not(feature = "web-fetch"))]
            assert!(stream_id(js_readable_stream_from_response(0.0)) >= STREAM_HANDLE_ID_START);
            assert_eq!(stream_id(STREAM_HANDLE_ID_START as f64), 0);
        }
    }
}
