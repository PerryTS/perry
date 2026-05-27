//! Retain `node:stream` runtime entry points across whole-program optimization.
//!
//! Codegen references these `#[no_mangle]` exports by symbol name from stream
//! dispatch paths, so Rust's crate graph does not always contain a direct call
//! edge. The auto-optimize build can otherwise internalize and strip an export.

use crate::node_stream::{
    js_node_stream_add_abort_signal, js_node_stream_compose, js_node_stream_duplex_new,
    js_node_stream_duplex_pair, js_node_stream_from_web, js_node_stream_get_default_hwm,
    js_node_stream_is_disturbed, js_node_stream_is_errored, js_node_stream_is_readable,
    js_node_stream_is_writable, js_node_stream_method_destroy, js_node_stream_method_emit,
    js_node_stream_method_end, js_node_stream_method_push, js_node_stream_method_read,
    js_node_stream_method_readable_hwm, js_node_stream_method_resume,
    js_node_stream_method_writable_hwm, js_node_stream_method_write,
    js_node_stream_passthrough_new, js_node_stream_readable_from, js_node_stream_readable_new,
    js_node_stream_set_default_hwm, js_node_stream_to_web, js_node_stream_transform_new,
    js_node_stream_writable_new,
};

#[used]
static KEEP_NS_METHOD_EMIT: extern "C" fn(i64, f64, f64) -> f64 = js_node_stream_method_emit;
#[used]
static KEEP_NS_METHOD_READ: extern "C" fn(i64, f64) -> f64 = js_node_stream_method_read;
#[used]
static KEEP_NS_METHOD_PUSH: extern "C" fn(i64, f64) -> f64 = js_node_stream_method_push;
#[used]
static KEEP_NS_READABLE_HWM: extern "C" fn(i64) -> f64 = js_node_stream_method_readable_hwm;
#[used]
static KEEP_NS_WRITABLE_HWM: extern "C" fn(i64) -> f64 = js_node_stream_method_writable_hwm;
#[used]
static KEEP_NS_METHOD_RESUME: extern "C" fn(i64) -> f64 = js_node_stream_method_resume;
#[used]
static KEEP_NS_METHOD_DESTROY: extern "C" fn(i64, f64) -> f64 = js_node_stream_method_destroy;
#[used]
static KEEP_NS_METHOD_WRITE: extern "C" fn(i64, f64, f64) -> f64 = js_node_stream_method_write;
#[used]
static KEEP_NS_METHOD_END: extern "C" fn(i64, f64) -> f64 = js_node_stream_method_end;
#[used]
static KEEP_NS_READABLE_NEW: extern "C" fn(f64) -> f64 = js_node_stream_readable_new;
#[used]
static KEEP_NS_WRITABLE_NEW: extern "C" fn(f64) -> f64 = js_node_stream_writable_new;
#[used]
static KEEP_NS_DUPLEX_NEW: extern "C" fn(f64) -> f64 = js_node_stream_duplex_new;
#[used]
static KEEP_NS_TRANSFORM_NEW: extern "C" fn(f64) -> f64 = js_node_stream_transform_new;
#[used]
static KEEP_NS_PASSTHROUGH_NEW: extern "C" fn(f64) -> f64 = js_node_stream_passthrough_new;
#[used]
static KEEP_NS_READABLE_FROM: extern "C" fn(f64) -> f64 = js_node_stream_readable_from;
#[used]
static KEEP_NS_IS_DISTURBED: extern "C" fn(f64) -> f64 = js_node_stream_is_disturbed;
#[used]
static KEEP_NS_IS_ERRORED: extern "C" fn(f64) -> f64 = js_node_stream_is_errored;
#[used]
static KEEP_NS_IS_READABLE: extern "C" fn(f64) -> f64 = js_node_stream_is_readable;
#[used]
static KEEP_NS_IS_WRITABLE: extern "C" fn(f64) -> f64 = js_node_stream_is_writable;
#[used]
static KEEP_NS_GET_DEFAULT_HWM: extern "C" fn(f64) -> f64 = js_node_stream_get_default_hwm;
#[used]
static KEEP_NS_SET_DEFAULT_HWM: extern "C" fn(f64, f64) -> f64 = js_node_stream_set_default_hwm;
#[used]
static KEEP_NS_ADD_ABORT_SIGNAL: extern "C" fn(f64, f64) -> f64 = js_node_stream_add_abort_signal;
#[used]
static KEEP_NS_COMPOSE: extern "C" fn(f64) -> f64 = js_node_stream_compose;
#[used]
static KEEP_NS_DUPLEX_PAIR: extern "C" fn(f64) -> f64 = js_node_stream_duplex_pair;
#[used]
static KEEP_NS_TO_WEB: extern "C" fn(f64) -> f64 = js_node_stream_to_web;
#[used]
static KEEP_NS_FROM_WEB: extern "C" fn(f64) -> f64 = js_node_stream_from_web;
