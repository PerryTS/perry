//! node:stream — constructors, init/option-parsing, and module-level FFI entry points (split out of node_stream.rs for the 2000-line
//! file-size gate, #1987). Shares the parent module's constants, hidden-key
//! accessors and state primitives via `use super::*`.
use super::*;
use crate::value::JSValue;

thread_local! {
    static ITER_HELPER_ARITIES_REGISTERED: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

/// Register declared arities for the iterator-helper stubs (once per
/// thread) so the closure dispatcher pads missing trailing args with
/// `undefined` instead of reading register garbage. `reduce` strictly
/// needs it — `reduce(fn)` omits the initial value — and registering
/// the single-arg helpers makes a missing-callback call (`map()`)
/// degrade to a no-op rather than dereference junk.
pub(super) fn register_iter_helper_arities() {
    if ITER_HELPER_ARITIES_REGISTERED.with(|c| c.replace(true)) {
        return;
    }
    let entries: &[(StubFn, u32)] = &[
        (cast1(ns_iter_to_array), 1),
        (cast2(ns_iter_map), 2),
        (cast2(ns_iter_filter), 2),
        (cast3(ns_iter_reduce), 3),
        (cast2(ns_iter_for_each), 2),
        (cast2(ns_iter_find), 2),
        (cast2(ns_iter_some), 2),
        (cast2(ns_iter_every), 2),
        (cast2(ns_iter_flat_map), 2),
        (cast1(ns_iter_take), 1),
        (cast1(ns_iter_drop), 1),
    ];
    for (f, arity) in entries {
        crate::closure::js_register_closure_arity(*f as *const u8, *arity);
    }
}

/// Coerce a NaN-boxed value to an `f64` if it is numeric (handling both the
/// int32-boxed and double representations). Returns `None` for non-numbers.
pub(super) fn jsvalue_as_f64(v: f64) -> Option<f64> {
    let jsval = JSValue::from_bits(v.to_bits());
    if jsval.is_int32() {
        Some(jsval.as_int32() as f64)
    } else if jsval.is_number() {
        Some(jsval.as_number())
    } else {
        None
    }
}

/// Read a numeric constructor option (e.g. `highWaterMark`) off the opts
/// object, returning `None` when absent or non-numeric.
pub(super) fn opt_number(opts: f64, key: &[u8]) -> Option<f64> {
    jsvalue_as_f64(get_hidden_value(opts, hidden_key(key))?)
}

/// Read a string constructor option and preserve the existing JS string value.
pub(super) fn opt_string_value(opts: f64, key: &[u8]) -> Option<f64> {
    let value = get_hidden_value(opts, hidden_key(key))?;
    if JSValue::from_bits(value.to_bits()).is_any_string() {
        Some(value)
    } else {
        None
    }
}

/// Read a boolean constructor option, returning `true` only when the option
/// is present and truthy.
pub(super) fn opt_bool(opts: f64, key: &[u8]) -> bool {
    get_hidden_value(opts, hidden_key(key)).is_some_and(|v| crate::value::js_is_truthy(v) != 0)
}

pub(super) fn resolve_object_mode(opts: f64, specific_object_mode: &[u8]) -> bool {
    opt_bool(opts, specific_object_mode) || opt_bool(opts, b"objectMode")
}

// #1537: the platform-default highWaterMark, settable at runtime via
// `stream.setDefaultHighWaterMark(objectMode, value)`. Node's defaults are
// 65536 bytes for byte streams and 16 for objectMode; both are mutable for
// the lifetime of the process (Perry tracks them per-thread, matching its
// per-thread runtime model). Streams constructed without an explicit
// `highWaterMark` inherit the current default for their mode.
thread_local! {
    static DEFAULT_HWM_BYTE: std::cell::Cell<f64> = const { std::cell::Cell::new(65536.0) };
    static DEFAULT_HWM_OBJECT: std::cell::Cell<f64> = const { std::cell::Cell::new(16.0) };
}

pub(super) fn default_hwm(object_mode: bool) -> f64 {
    if object_mode {
        DEFAULT_HWM_OBJECT.with(|c| c.get())
    } else {
        DEFAULT_HWM_BYTE.with(|c| c.get())
    }
}

/// Resolve an effective highWaterMark: the direction-specific option
/// (`readableHighWaterMark` / `writableHighWaterMark`) falls back to the
/// generic `highWaterMark`, then to the platform default for the stream's
/// mode (#1537: 65536 for byte streams, 16 for objectMode).
pub(super) fn resolve_hwm(opts: f64, specific: &[u8], specific_object_mode: &[u8]) -> f64 {
    if let Some(v) = opt_number(opts, specific).or_else(|| opt_number(opts, b"highWaterMark")) {
        return v;
    }
    let object_mode = resolve_object_mode(opts, specific_object_mode);
    default_hwm(object_mode)
}

/// Initialize visible lifecycle flags shared by all stream sides.
pub(super) fn init_lifecycle_state(stream: f64, opts: f64) {
    set_hidden_value(stream, hidden_key(b"destroyed"), f64::from_bits(TAG_FALSE));
    set_stream_emit_close(stream, opts);
    set_hidden_value(
        stream,
        hidden_capture_rejections_key(),
        f64::from_bits(if opt_bool(opts, b"captureRejections") {
            TAG_TRUE
        } else {
            TAG_FALSE
        }),
    );
    set_visible_closed(stream, false);
}

pub(super) fn init_constructor(stream: f64, name: &str) {
    let constructor = crate::object::bound_native_callable_export_value("stream", name);
    set_hidden_value(stream, hidden_key(b"constructor"), constructor);
}

pub(super) fn set_visible_readable(stream: f64, readable: bool) {
    if get_hidden_value(stream, hidden_readable_flag_key()).is_some() {
        let value = if readable { TAG_TRUE } else { TAG_FALSE };
        set_hidden_value(stream, hidden_key(b"readable"), f64::from_bits(value));
    }
}

pub(super) fn set_visible_readable_ended(stream: f64, ended: bool) {
    if get_hidden_value(stream, hidden_readable_flag_key()).is_some() {
        let value = if ended { TAG_TRUE } else { TAG_FALSE };
        set_hidden_value(stream, hidden_key(b"readableEnded"), f64::from_bits(value));
    }
}

pub(super) fn set_visible_readable_did_read(stream: f64, did_read: bool) {
    if get_hidden_value(stream, hidden_readable_flag_key()).is_some() {
        let value = if did_read { TAG_TRUE } else { TAG_FALSE };
        set_hidden_value(
            stream,
            hidden_key(b"readableDidRead"),
            f64::from_bits(value),
        );
    }
}

pub(super) fn readable_encoding_value(stream: f64) -> f64 {
    get_hidden_value(stream, hidden_key(b"readableEncoding")).unwrap_or(f64::from_bits(TAG_NULL))
}

pub(super) fn normalize_readable_encoding(encoding: f64) -> f64 {
    if JSValue::from_bits(encoding.to_bits()).is_any_string() {
        encoding
    } else {
        f64::from_bits(TAG_NULL)
    }
}

pub(super) fn set_visible_readable_encoding(stream: f64, encoding: f64) {
    if get_hidden_value(stream, hidden_readable_flag_key()).is_some() {
        set_hidden_value(stream, hidden_key(b"readableEncoding"), encoding);
    }
}

pub(super) fn mark_stream_ended(stream: f64) {
    set_hidden_value(stream, hidden_ended_key(), f64::from_bits(TAG_TRUE));
    set_visible_readable(stream, false);
    set_visible_readable_ended(stream, true);
}

pub(super) fn set_visible_writable(stream: f64, writable: bool) {
    if get_hidden_value(stream, hidden_writable_flag_key()).is_some() {
        let value = if writable { TAG_TRUE } else { TAG_FALSE };
        set_hidden_value(stream, hidden_key(b"writable"), f64::from_bits(value));
    }
}

pub(super) fn set_visible_writable_ended(stream: f64, ended: bool) {
    if get_hidden_value(stream, hidden_writable_flag_key()).is_some() {
        let value = if ended { TAG_TRUE } else { TAG_FALSE };
        set_hidden_value(stream, hidden_key(b"writableEnded"), f64::from_bits(value));
    }
}

pub(super) fn set_visible_writable_finished(stream: f64, finished: bool) {
    if get_hidden_value(stream, hidden_writable_flag_key()).is_some() {
        let value = if finished { TAG_TRUE } else { TAG_FALSE };
        set_hidden_value(
            stream,
            hidden_key(b"writableFinished"),
            f64::from_bits(value),
        );
    }
}

pub(super) fn mark_writable_ended(stream: f64) {
    set_hidden_value(stream, hidden_ended_key(), f64::from_bits(TAG_TRUE));
    set_visible_writable(stream, false);
    set_visible_writable_ended(stream, true);
}

pub(super) fn mark_writable_finished(stream: f64) {
    set_visible_writable(stream, false);
    set_visible_writable_finished(stream, true);
}

pub(super) fn set_visible_closed(stream: f64, closed: bool) {
    let value = if closed { TAG_TRUE } else { TAG_FALSE };
    set_hidden_value(stream, hidden_key(b"closed"), f64::from_bits(value));
}

pub(super) fn mark_stream_closed(stream: f64) {
    set_visible_closed(stream, true);
}

/// Initialize the readable side of a stream: direction flag, buffered byte
/// counter, effective readable highWaterMark, and the visible
/// `readableHighWaterMark` / `destroyed` properties (#1534/#1539).
pub(super) fn init_readable_state(stream: f64, opts: f64) {
    set_stream_auto_destroy(stream, opts);
    set_hidden_value(stream, hidden_readable_flag_key(), f64::from_bits(TAG_TRUE));
    set_hidden_value(stream, hidden_key(b"destroyed"), f64::from_bits(TAG_FALSE));
    set_hidden_value(
        stream,
        hidden_key(b"readableAborted"),
        f64::from_bits(TAG_FALSE),
    );
    set_hidden_value(stream, hidden_buffered_key(), 0.0);
    set_hidden_value(stream, hidden_key(b"readableLength"), 0.0);
    let readable_object_mode = resolve_object_mode(opts, b"readableObjectMode");
    set_hidden_value(
        stream,
        hidden_key(b"readableObjectMode"),
        f64::from_bits(if readable_object_mode {
            TAG_TRUE
        } else {
            TAG_FALSE
        }),
    );
    let r_hwm = resolve_hwm(opts, b"readableHighWaterMark", b"readableObjectMode");
    set_hidden_value(stream, hidden_hwm_key(), r_hwm);
    set_hidden_value(stream, hidden_key(b"readableHighWaterMark"), r_hwm);
    set_hidden_value(stream, readable_flowing_key(), f64::from_bits(TAG_NULL));
    set_hidden_value(
        stream,
        hidden_readable_pending_key(),
        box_pointer(crate::array::js_array_alloc(0) as *const u8),
    );
    set_hidden_value(
        stream,
        hidden_stream_pipes_key(),
        box_pointer(crate::array::js_array_alloc(0) as *const u8),
    );
    set_visible_readable(stream, true);
    set_visible_readable_ended(stream, false);
    set_visible_readable_did_read(stream, false);
    let encoding = opt_string_value(opts, b"encoding").unwrap_or(f64::from_bits(TAG_NULL));
    set_visible_readable_encoding(stream, encoding);
}

/// Initialize the writable side: direction flag and visible stream flags.
pub(super) fn init_writable_state(stream: f64, opts: f64) {
    set_stream_auto_destroy(stream, opts);
    set_hidden_value(stream, hidden_writable_flag_key(), f64::from_bits(TAG_TRUE));
    set_hidden_value(stream, hidden_key(b"destroyed"), f64::from_bits(TAG_FALSE));
    let writable_object_mode = resolve_object_mode(opts, b"writableObjectMode");
    set_hidden_value(
        stream,
        hidden_key(b"writableObjectMode"),
        f64::from_bits(if writable_object_mode {
            TAG_TRUE
        } else {
            TAG_FALSE
        }),
    );
    let w_hwm = resolve_hwm(opts, b"writableHighWaterMark", b"writableObjectMode");
    set_hidden_value(stream, hidden_key(b"writableHighWaterMark"), w_hwm);
    set_hidden_value(
        stream,
        hidden_writable_object_mode_key(),
        f64::from_bits(if writable_object_mode {
            TAG_TRUE
        } else {
            TAG_FALSE
        }),
    );
    let decode_strings = get_hidden_value(opts, hidden_key(b"decodeStrings"))
        .is_none_or(|v| v.to_bits() != TAG_FALSE);
    set_hidden_value(
        stream,
        hidden_writable_decode_strings_key(),
        f64::from_bits(if decode_strings { TAG_TRUE } else { TAG_FALSE }),
    );
    let default_encoding =
        opt_string_value(opts, b"defaultEncoding").unwrap_or_else(|| string_value(b"utf8"));
    set_hidden_value(
        stream,
        hidden_writable_default_encoding_key(),
        default_encoding,
    );
    set_writable_length(stream, 0.0);
    set_writable_need_drain(stream, false);
    set_pending_writable_finish_callback(stream, None);
    set_writable_corked_count(stream, 0.0);
    set_hidden_value(
        stream,
        hidden_writable_buffered_key(),
        box_pointer(crate::array::js_array_alloc(0) as *const u8),
    );
    set_visible_writable(stream, true);
    set_visible_writable_ended(stream, false);
    set_visible_writable_finished(stream, false);
}

pub(super) fn init_duplex_state(stream: f64, opts: f64) {
    let allow_half_open = if get_hidden_value(opts, hidden_key(b"allowHalfOpen"))
        .is_some_and(|v| v.to_bits() == TAG_FALSE)
    {
        TAG_FALSE
    } else {
        TAG_TRUE
    };
    set_hidden_value(
        stream,
        hidden_key(b"allowHalfOpen"),
        f64::from_bits(allow_half_open),
    );
}

pub(super) fn init_abort_signal_state(stream: f64, opts: f64) {
    if let Some(signal) = options_signal(opts) {
        attach_abort_signal(signal, stream);
    }
}

// ─────────────────────────────────────────────────────────────────
// #1987: the body of this module is split into topical siblings to stay
// under the 2000-line file-size gate. The constants, hidden-key accessors,
// option-parsing helpers and state primitives above are shared with each
// sibling via `use super::*`. Items referenced through the parent module's
// `pub use constructors::*` glob are re-exported by name below.
// ─────────────────────────────────────────────────────────────────

#[path = "node_stream_constructors/builders.rs"]
mod builders;
#[path = "node_stream_constructors/introspection.rs"]
mod introspection;
#[path = "node_stream_constructors/pipeline.rs"]
mod pipeline;
#[path = "node_stream_constructors/web_adapter.rs"]
mod web_adapter;

pub use builders::{
    js_array_subclass_init, js_event_emitter_subclass_init, js_node_stream_duplex_new,
    js_node_stream_duplex_subclass_init, js_node_stream_passthrough_new,
    js_node_stream_readable_from, js_node_stream_readable_from_options,
    js_node_stream_readable_new, js_node_stream_readable_subclass_init,
    js_node_stream_transform_new, js_node_stream_transform_subclass_init,
    js_node_stream_writable_new, js_node_stream_writable_subclass_init,
};

pub use introspection::{
    js_node_stream_add_abort_signal, js_node_stream_get_default_hwm,
    js_node_stream_is_array_buffer_view, js_node_stream_is_destroyed, js_node_stream_is_disturbed,
    js_node_stream_is_errored, js_node_stream_is_readable, js_node_stream_is_uint8_array,
    js_node_stream_is_writable, js_node_stream_set_default_hwm,
    js_node_stream_uint8_array_to_buffer,
};
// `attach_abort_signal` (introspection) is called by `init_abort_signal_state`
// in this trunk; `bool_value` (introspection) is reached by `web_adapter` via
// `use super::*`. Re-export both so those paths resolve.
pub(crate) use introspection::{attach_abort_signal, bool_value};

pub use pipeline::{
    js_node_stream_compose, js_node_stream_compose_args, js_node_stream_duplex_from_options,
    js_node_stream_duplex_pair, js_node_stream_finished, js_node_stream_pipeline,
};
// Duplex-pair write/final callbacks are registered by the dispatch module
// (`node_stream_dispatch.rs`); surface them through the constructors trunk so
// `node_stream`'s `pub use constructors::*` carries them into that module.
pub(crate) use pipeline::{duplex_pair_final_callback, duplex_pair_write_callback};

pub use web_adapter::{
    js_node_stream_duplex_from_web, js_node_stream_duplex_to_web, js_node_stream_from_web,
    js_node_stream_readable_from_web, js_node_stream_readable_to_web, js_node_stream_to_web,
    js_node_stream_writable_from_web, js_node_stream_writable_to_web,
    js_register_node_stream_web_adapter_callbacks,
};
pub(crate) use web_adapter::{
    js_node_stream_duplex_to_web_method_value, js_node_stream_readable_to_web_method_value,
    js_node_stream_writable_to_web_method_value,
};
