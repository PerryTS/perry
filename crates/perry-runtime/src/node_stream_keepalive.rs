// #1534/#1539/#1540/#1541: symbol retention.
//
// These `#[no_mangle]` entry points are emitted by codegen's stream
// dispatch (native_table/net_events.rs) but several are never referenced
// by any Rust code in the crate graph. The default `.a` staticlib keeps
// them via staticlib-export semantics, but the auto-optimize build round-
// trips the runtime through whole-program LLVM bitcode and is free to
// internalize and dead-strip an unreferenced symbol. The `#[used]` statics
// below pin retained reference edges so every entry point survives all link
// modes. See the same pattern in `value/dyn_index.rs` and `process.rs`.

#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_EMIT: extern "C" fn(i64, f64, f64) -> f64 = super::js_node_stream_method_emit;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_EMIT_ARGS: extern "C" fn(i64, f64, i64) -> f64 =
    super::js_node_stream_method_emit_args;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_READ: extern "C" fn(i64, f64) -> f64 = super::js_node_stream_method_read;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_PUSH: extern "C" fn(i64, f64) -> f64 = super::js_node_stream_method_push;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_UNSHIFT: extern "C" fn(i64, f64) -> f64 =
    super::js_node_stream_method_unshift;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_READABLE_HWM: extern "C" fn(i64) -> f64 = super::js_node_stream_method_readable_hwm;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_READABLE_LENGTH: extern "C" fn(i64) -> f64 =
    super::js_node_stream_method_readable_length;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_READABLE_OBJECT_MODE: extern "C" fn(i64) -> f64 =
    super::js_node_stream_method_readable_object_mode;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_READABLE: extern "C" fn(i64) -> f64 = super::js_node_stream_method_readable;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_READABLE_ENDED: extern "C" fn(i64) -> f64 =
    super::js_node_stream_method_readable_ended;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_READABLE_ENCODING: extern "C" fn(i64) -> f64 =
    super::js_node_stream_method_readable_encoding;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_WRITABLE_HWM: extern "C" fn(i64) -> f64 = super::js_node_stream_method_writable_hwm;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_WRITABLE_LENGTH: extern "C" fn(i64) -> f64 =
    super::js_node_stream_method_writable_length;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_WRITABLE_NEED_DRAIN: extern "C" fn(i64) -> f64 =
    super::js_node_stream_method_writable_need_drain;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_WRITABLE_OBJECT_MODE: extern "C" fn(i64) -> f64 =
    super::js_node_stream_method_writable_object_mode;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_READABLE_ABORTED: extern "C" fn(i64) -> f64 =
    super::js_node_stream_method_readable_aborted;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_CLOSED: extern "C" fn(i64) -> f64 = super::js_node_stream_method_closed;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_ERRORED: extern "C" fn(i64) -> f64 = super::js_node_stream_method_errored;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_READABLE_DID_READ: extern "C" fn(i64) -> f64 =
    super::js_node_stream_method_readable_did_read;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_WRITABLE_CORKED: extern "C" fn(i64) -> f64 =
    super::js_node_stream_method_writable_corked;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_WRITABLE: extern "C" fn(i64) -> f64 = super::js_node_stream_method_writable;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_WRITABLE_ENDED: extern "C" fn(i64) -> f64 =
    super::js_node_stream_method_writable_ended;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_WRITABLE_FINISHED: extern "C" fn(i64) -> f64 =
    super::js_node_stream_method_writable_finished;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_ALLOW_HALF_OPEN: extern "C" fn(i64) -> f64 =
    super::js_node_stream_method_allow_half_open;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_PAUSE: extern "C" fn(i64) -> f64 = super::js_node_stream_method_pause;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_RESUME: extern "C" fn(i64) -> f64 = super::js_node_stream_method_resume;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_SET_ENCODING: extern "C" fn(i64, f64) -> f64 =
    super::js_node_stream_method_set_encoding;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_DESTROY: extern "C" fn(i64, f64) -> f64 =
    super::js_node_stream_method_destroy;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_DESTROYED: extern "C" fn(i64) -> f64 = super::js_node_stream_method_destroyed;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_WRITE: extern "C" fn(i64, f64, f64, f64) -> f64 =
    super::js_node_stream_method_write;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_END: extern "C" fn(i64, f64) -> f64 = super::js_node_stream_method_end;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_END3: extern "C" fn(i64, f64, f64, f64) -> f64 =
    super::js_node_stream_method_end3;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_CORK: extern "C" fn(i64) -> f64 = super::js_node_stream_method_cork;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_UNCORK: extern "C" fn(i64) -> f64 = super::js_node_stream_method_uncork;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_SET_MAX_LISTENERS: extern "C" fn(i64, f64) -> f64 =
    super::js_node_stream_method_set_max_listeners;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_GET_MAX_LISTENERS: extern "C" fn(i64) -> f64 =
    super::js_node_stream_method_get_max_listeners;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_ON: extern "C" fn(i64, f64, f64) -> f64 = super::js_node_stream_method_on;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_ONCE: extern "C" fn(i64, f64, f64) -> f64 = super::js_node_stream_method_once;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_PREPEND_LISTENER: extern "C" fn(i64, f64, f64) -> f64 =
    super::js_node_stream_method_prepend_listener;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_PREPEND_ONCE_LISTENER: extern "C" fn(i64, f64, f64) -> f64 =
    super::js_node_stream_method_prepend_once_listener;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_OFF: extern "C" fn(i64, f64, f64) -> f64 = super::js_node_stream_method_off;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_REMOVE_LISTENER: extern "C" fn(i64, f64, f64) -> f64 =
    super::js_node_stream_method_remove_listener;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_REMOVE_ALL_LISTENERS: extern "C" fn(i64, f64) -> f64 =
    super::js_node_stream_method_remove_all_listeners;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_EVENT_NAMES: extern "C" fn(i64) -> i64 =
    super::js_node_stream_method_event_names;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_LISTENER_COUNT: extern "C" fn(i64, f64) -> f64 =
    super::js_node_stream_method_listener_count;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_LISTENERS: extern "C" fn(i64, f64) -> i64 =
    super::js_node_stream_method_listeners;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_METHOD_RAW_LISTENERS: extern "C" fn(i64, f64) -> i64 =
    super::js_node_stream_method_raw_listeners;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_READABLE_NEW: extern "C" fn(f64) -> f64 = super::js_node_stream_readable_new;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_WRITABLE_NEW: extern "C" fn(f64) -> f64 = super::js_node_stream_writable_new;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_DUPLEX_NEW: extern "C" fn(f64) -> f64 = super::js_node_stream_duplex_new;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_TRANSFORM_NEW: extern "C" fn(f64) -> f64 = super::js_node_stream_transform_new;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_PASSTHROUGH_NEW: extern "C" fn(f64) -> f64 = super::js_node_stream_passthrough_new;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_READABLE_FROM: extern "C" fn(f64) -> f64 = super::js_node_stream_readable_from;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_READABLE_FROM_OPTIONS: extern "C" fn(f64, f64) -> f64 =
    super::js_node_stream_readable_from_options;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_IS_DISTURBED: extern "C" fn(f64) -> f64 = super::js_node_stream_is_disturbed;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_IS_ERRORED: extern "C" fn(f64) -> f64 = super::js_node_stream_is_errored;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_IS_READABLE: extern "C" fn(f64) -> f64 = super::js_node_stream_is_readable;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_IS_WRITABLE: extern "C" fn(f64) -> f64 = super::js_node_stream_is_writable;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_IS_ARRAY_BUFFER_VIEW: extern "C" fn(f64) -> f64 =
    super::js_node_stream_is_array_buffer_view;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_IS_UINT8_ARRAY: extern "C" fn(f64) -> f64 = super::js_node_stream_is_uint8_array;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_IS_DESTROYED: extern "C" fn(f64) -> f64 = super::js_node_stream_is_destroyed;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_UINT8_ARRAY_TO_BUFFER: extern "C" fn(f64) -> f64 =
    super::js_node_stream_uint8_array_to_buffer;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_GET_DEFAULT_HWM: extern "C" fn(f64) -> f64 = super::js_node_stream_get_default_hwm;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_SET_DEFAULT_HWM: extern "C" fn(f64, f64) -> f64 =
    super::js_node_stream_set_default_hwm;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_ADD_ABORT_SIGNAL: extern "C" fn(f64, f64) -> f64 =
    super::js_node_stream_add_abort_signal;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_COMPOSE: extern "C" fn(*const crate::array::ArrayHeader) -> f64 =
    super::js_node_stream_compose;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_PIPELINE: extern "C" fn(*const crate::array::ArrayHeader) -> f64 =
    super::js_node_stream_pipeline;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_DUPLEX_PAIR: extern "C" fn(f64) -> f64 = super::js_node_stream_duplex_pair;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_TO_WEB: extern "C" fn(f64) -> f64 = super::js_node_stream_to_web;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_NS_FROM_WEB: extern "C" fn(f64) -> f64 = super::js_node_stream_from_web;
