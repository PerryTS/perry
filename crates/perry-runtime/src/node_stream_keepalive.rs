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

#[used]
static KEEP_NS_METHOD_EMIT: extern "C" fn(i64, f64, f64) -> f64 = super::js_node_stream_method_emit;
#[used]
static KEEP_NS_METHOD_EMIT_ARGS: extern "C" fn(i64, f64, i64) -> f64 =
    super::js_node_stream_method_emit_args;
#[used]
static KEEP_NS_METHOD_READ: extern "C" fn(i64, f64) -> f64 = super::js_node_stream_method_read;
#[used]
static KEEP_NS_METHOD_PUSH: extern "C" fn(i64, f64) -> f64 = super::js_node_stream_method_push;
#[used]
static KEEP_NS_READABLE_HWM: extern "C" fn(i64) -> f64 = super::js_node_stream_method_readable_hwm;
#[used]
static KEEP_NS_METHOD_READABLE: extern "C" fn(i64) -> f64 = super::js_node_stream_method_readable;
#[used]
static KEEP_NS_METHOD_READABLE_ENDED: extern "C" fn(i64) -> f64 =
    super::js_node_stream_method_readable_ended;
#[used]
static KEEP_NS_WRITABLE_HWM: extern "C" fn(i64) -> f64 = super::js_node_stream_method_writable_hwm;
#[used]
static KEEP_NS_READABLE_ABORTED: extern "C" fn(i64) -> f64 =
    super::js_node_stream_method_readable_aborted;
#[used]
static KEEP_NS_WRITABLE_CORKED: extern "C" fn(i64) -> f64 =
    super::js_node_stream_method_writable_corked;
#[used]
static KEEP_NS_METHOD_WRITABLE: extern "C" fn(i64) -> f64 = super::js_node_stream_method_writable;
#[used]
static KEEP_NS_METHOD_WRITABLE_ENDED: extern "C" fn(i64) -> f64 =
    super::js_node_stream_method_writable_ended;
#[used]
static KEEP_NS_METHOD_WRITABLE_FINISHED: extern "C" fn(i64) -> f64 =
    super::js_node_stream_method_writable_finished;
#[used]
static KEEP_NS_METHOD_RESUME: extern "C" fn(i64) -> f64 = super::js_node_stream_method_resume;
#[used]
static KEEP_NS_METHOD_DESTROY: extern "C" fn(i64, f64) -> f64 =
    super::js_node_stream_method_destroy;
#[used]
static KEEP_NS_METHOD_DESTROYED: extern "C" fn(i64) -> f64 = super::js_node_stream_method_destroyed;
#[used]
static KEEP_NS_METHOD_WRITE: extern "C" fn(i64, f64, f64) -> f64 =
    super::js_node_stream_method_write;
#[used]
static KEEP_NS_METHOD_END: extern "C" fn(i64, f64) -> f64 = super::js_node_stream_method_end;
#[used]
static KEEP_NS_METHOD_END3: extern "C" fn(i64, f64, f64, f64) -> f64 =
    super::js_node_stream_method_end3;
#[used]
static KEEP_NS_METHOD_CORK: extern "C" fn(i64) -> f64 = super::js_node_stream_method_cork;
#[used]
static KEEP_NS_METHOD_UNCORK: extern "C" fn(i64) -> f64 = super::js_node_stream_method_uncork;
#[used]
static KEEP_NS_METHOD_SET_MAX_LISTENERS: extern "C" fn(i64, f64) -> f64 =
    super::js_node_stream_method_set_max_listeners;
#[used]
static KEEP_NS_METHOD_GET_MAX_LISTENERS: extern "C" fn(i64) -> f64 =
    super::js_node_stream_method_get_max_listeners;
#[used]
static KEEP_NS_METHOD_ON: extern "C" fn(i64, f64, f64) -> f64 = super::js_node_stream_method_on;
#[used]
static KEEP_NS_METHOD_ONCE: extern "C" fn(i64, f64, f64) -> f64 = super::js_node_stream_method_once;
#[used]
static KEEP_NS_METHOD_PREPEND_LISTENER: extern "C" fn(i64, f64, f64) -> f64 =
    super::js_node_stream_method_prepend_listener;
#[used]
static KEEP_NS_METHOD_PREPEND_ONCE_LISTENER: extern "C" fn(i64, f64, f64) -> f64 =
    super::js_node_stream_method_prepend_once_listener;
#[used]
static KEEP_NS_METHOD_OFF: extern "C" fn(i64, f64, f64) -> f64 = super::js_node_stream_method_off;
#[used]
static KEEP_NS_METHOD_REMOVE_LISTENER: extern "C" fn(i64, f64, f64) -> f64 =
    super::js_node_stream_method_remove_listener;
#[used]
static KEEP_NS_METHOD_REMOVE_ALL_LISTENERS: extern "C" fn(i64, f64) -> f64 =
    super::js_node_stream_method_remove_all_listeners;
#[used]
static KEEP_NS_METHOD_EVENT_NAMES: extern "C" fn(i64) -> i64 =
    super::js_node_stream_method_event_names;
#[used]
static KEEP_NS_METHOD_LISTENER_COUNT: extern "C" fn(i64, f64) -> f64 =
    super::js_node_stream_method_listener_count;
#[used]
static KEEP_NS_METHOD_LISTENERS: extern "C" fn(i64, f64) -> i64 =
    super::js_node_stream_method_listeners;
#[used]
static KEEP_NS_METHOD_RAW_LISTENERS: extern "C" fn(i64, f64) -> i64 =
    super::js_node_stream_method_raw_listeners;
#[used]
static KEEP_NS_READABLE_NEW: extern "C" fn(f64) -> f64 = super::js_node_stream_readable_new;
#[used]
static KEEP_NS_WRITABLE_NEW: extern "C" fn(f64) -> f64 = super::js_node_stream_writable_new;
#[used]
static KEEP_NS_DUPLEX_NEW: extern "C" fn(f64) -> f64 = super::js_node_stream_duplex_new;
#[used]
static KEEP_NS_TRANSFORM_NEW: extern "C" fn(f64) -> f64 = super::js_node_stream_transform_new;
#[used]
static KEEP_NS_PASSTHROUGH_NEW: extern "C" fn(f64) -> f64 = super::js_node_stream_passthrough_new;
#[used]
static KEEP_NS_READABLE_FROM: extern "C" fn(f64) -> f64 = super::js_node_stream_readable_from;
#[used]
static KEEP_NS_IS_DISTURBED: extern "C" fn(f64) -> f64 = super::js_node_stream_is_disturbed;
#[used]
static KEEP_NS_IS_ERRORED: extern "C" fn(f64) -> f64 = super::js_node_stream_is_errored;
#[used]
static KEEP_NS_IS_READABLE: extern "C" fn(f64) -> f64 = super::js_node_stream_is_readable;
#[used]
static KEEP_NS_IS_WRITABLE: extern "C" fn(f64) -> f64 = super::js_node_stream_is_writable;
#[used]
static KEEP_NS_GET_DEFAULT_HWM: extern "C" fn(f64) -> f64 = super::js_node_stream_get_default_hwm;
#[used]
static KEEP_NS_SET_DEFAULT_HWM: extern "C" fn(f64, f64) -> f64 =
    super::js_node_stream_set_default_hwm;
#[used]
static KEEP_NS_ADD_ABORT_SIGNAL: extern "C" fn(f64, f64) -> f64 =
    super::js_node_stream_add_abort_signal;
#[used]
static KEEP_NS_COMPOSE: extern "C" fn(f64) -> f64 = super::js_node_stream_compose;
#[used]
static KEEP_NS_DUPLEX_PAIR: extern "C" fn(f64) -> f64 = super::js_node_stream_duplex_pair;
#[used]
static KEEP_NS_TO_WEB: extern "C" fn(f64) -> f64 = super::js_node_stream_to_web;
#[used]
static KEEP_NS_FROM_WEB: extern "C" fn(f64) -> f64 = super::js_node_stream_from_web;
