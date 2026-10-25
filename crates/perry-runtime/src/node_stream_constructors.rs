//! node:stream — constructors, init/option-parsing, and module-level FFI entry points (split out of node_stream.rs for the 2000-line
//! file-size gate, #1987). Shares the parent module's constants, hidden-key
//! accessors and state primitives via `use super::*`.
use super::*;
use crate::gc::RuntimeHandle;
use crate::value::JSValue;

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

/// Read a boolean constructor option, returning `true` only when the option
/// is present and truthy.
pub(super) fn opt_bool(opts: f64, key: crate::runtime_state_key::NamedStateKey) -> bool {
    get_hidden_value(opts, key).is_some_and(|v| crate::value::js_is_truthy(v) != 0)
}

pub(super) fn resolve_object_mode(
    opts: &RuntimeHandle<'_>,
    specific: crate::runtime_state_key::NamedStateKey,
    duplex: bool,
) -> bool {
    let generic = opt_bool(
        opts.get_nanbox_f64(),
        crate::runtime_state_key!(b"objectMode"),
    );
    let specific = duplex && opt_bool(opts.get_nanbox_f64(), specific);
    generic || specific
}

pub(super) fn normalized_hwm(
    opts: &RuntimeHandle<'_>,
    specific: crate::runtime_state_key::NamedStateKey,
    specific_name: &str,
    duplex: bool,
    object_mode: bool,
) -> f64 {
    let mut value = crate::runtime_state_key!(b"highWaterMark").read_value(opts.get_nanbox_f64());
    let mut name = "options.highWaterMark";
    let mut selected = crate::runtime_state_key!(b"highWaterMark");
    if value.to_bits() == TAG_UNDEFINED || value.to_bits() == TAG_NULL {
        if duplex {
            value = specific.read_value(opts.get_nanbox_f64());
            name = specific_name;
            selected = specific;
        }
    }
    if value.to_bits() == TAG_UNDEFINED || value.to_bits() == TAG_NULL {
        #[cfg(test)]
        if std::env::var_os("PERRY_TEST_STREAM_MODE_REREAD").is_some() {
            return default_hwm(resolve_object_mode(opts, specific, duplex));
        }
        return default_hwm(object_mode);
    }
    value = selected.read_value(opts.get_nanbox_f64());
    if let Some(n) = jsvalue_as_f64(value) {
        if n.is_finite() && n >= 0.0 && n.floor() == n {
            return n;
        }
    }
    let message = format!(
        "The value of \"{name}\" is out of range. It must be >= 0. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE")
}

fn normalized_default_encoding(opts: &RuntimeHandle<'_>) -> f64 {
    let value = crate::runtime_state_key!(b"defaultEncoding").read_value(opts.get_nanbox_f64());
    if value.to_bits() == TAG_NULL || value.to_bits() == TAG_UNDEFINED {
        return literal_string_value(b"utf8");
    }
    validate_stream_encoding(value);
    value
}
fn validate_stream_encoding(value: f64) {
    if crate::buffer::js_buffer_is_encoding(value) == 0 {
        let scope = crate::gc::RuntimeHandleScope::new();
        let value = scope.root_nanbox_f64(value);
        let text = crate::value::js_jsvalue_to_string(value.get_nanbox_f64());
        let text = crate::fs::validate::read_js_string_pub(f64::from_bits(
            JSValue::string_ptr(text).bits(),
        ));
        crate::fs::validate::throw_type_error_with_code(
            &format!("Unknown encoding: {text}"),
            "ERR_UNKNOWN_ENCODING",
        );
    }
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

/// Constructor helpers borrow the builder's existing roots. Options can
/// invoke user getters and collect, so reload the receiver after each read.
/// Initialize visible lifecycle flags shared by all stream sides.
pub(super) fn init_lifecycle_state(stream: &RuntimeHandle<'_>, _: &RuntimeHandle<'_>) {
    super::state_record::initialize_slots(
        stream.get_nanbox_f64(),
        &[
            (Slot::Destroyed, f64::from_bits(TAG_FALSE)),
            (Slot::Closed, f64::from_bits(TAG_FALSE)),
        ],
        &[],
    );
}

pub(super) fn set_visible_readable(stream: f64, readable: bool) {
    if get_hidden_value(stream, hidden_readable_flag_key()).is_some() {
        let value = if readable { TAG_TRUE } else { TAG_FALSE };
        set_hidden_value(stream, Slot::Readable, f64::from_bits(value));
    }
}

pub(super) fn set_visible_readable_ended(stream: f64, ended: bool) {
    if get_hidden_value(stream, hidden_readable_flag_key()).is_some() {
        let value = if ended { TAG_TRUE } else { TAG_FALSE };
        set_hidden_value(stream, Slot::ReadableEnded, f64::from_bits(value));
    }
}

pub(super) fn set_visible_readable_did_read(stream: f64, did_read: bool) {
    if get_hidden_value(stream, hidden_readable_flag_key()).is_some() {
        let value = if did_read { TAG_TRUE } else { TAG_FALSE };
        set_hidden_value(stream, Slot::Disturbed, f64::from_bits(value));
    }
}

pub(super) fn readable_encoding_value(stream: f64) -> f64 {
    get_hidden_value(stream, Slot::ReadableEncoding).unwrap_or(f64::from_bits(TAG_NULL))
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
        set_hidden_value(stream, Slot::ReadableEncoding, encoding);
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
        set_hidden_value(stream, Slot::Writable, f64::from_bits(value));
    }
}

pub(super) fn set_visible_writable_ended(stream: f64, ended: bool) {
    if get_hidden_value(stream, hidden_writable_flag_key()).is_some() {
        let value = if ended { TAG_TRUE } else { TAG_FALSE };
        set_hidden_value(stream, Slot::WritableEnded, f64::from_bits(value));
    }
}

pub(super) fn set_visible_writable_finished(stream: f64, finished: bool) {
    if get_hidden_value(stream, hidden_writable_flag_key()).is_some() {
        let value = if finished { TAG_TRUE } else { TAG_FALSE };
        set_hidden_value(stream, Slot::WritableFinished, f64::from_bits(value));
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
    set_hidden_value(stream, Slot::Closed, f64::from_bits(value));
}

pub(super) fn mark_stream_closed(stream: f64) {
    set_visible_closed(stream, true);
}

/// Initialize the readable side of a stream: direction flag, buffered byte
/// counter, effective readable highWaterMark, and the visible
/// `readableHighWaterMark` / `destroyed` properties (#1534/#1539).
pub(super) fn init_readable_state(
    stream: &RuntimeHandle<'_>,
    opts: &RuntimeHandle<'_>,
    duplex: bool,
) {
    let mode = resolve_object_mode(
        opts,
        crate::runtime_state_key!(b"readableObjectMode"),
        duplex,
    );
    let hwm = normalized_hwm(
        opts,
        crate::runtime_state_key!(b"readableHighWaterMark"),
        "options.readableHighWaterMark",
        duplex,
        mode,
    );
    let emit_close = crate::runtime_state_key!(b"emitClose")
        .read_value(opts.get_nanbox_f64())
        .to_bits()
        != TAG_FALSE;
    let auto_destroy = crate::runtime_state_key!(b"autoDestroy")
        .read_value(opts.get_nanbox_f64())
        .to_bits()
        != TAG_FALSE;
    let scope = crate::gc::RuntimeHandleScope::new();
    let _default_encoding = scope.root_nanbox_f64(normalized_default_encoding(opts));
    let encoding = crate::runtime_state_key!(b"encoding").read_value(opts.get_nanbox_f64());
    let encoding = scope.root_nanbox_f64(if crate::value::js_is_truthy(encoding) != 0 {
        let decoder_encoding =
            crate::runtime_state_key!(b"encoding").read_value(opts.get_nanbox_f64());
        validate_stream_encoding(decoder_encoding);
        crate::runtime_state_key!(b"encoding").read_value(opts.get_nanbox_f64())
    } else {
        f64::from_bits(TAG_NULL)
    });
    let pipes = scope.root_nanbox_f64(box_pointer(crate::array::js_array_alloc(0).cast()));
    let bool_value = |on| f64::from_bits(if on { TAG_TRUE } else { TAG_FALSE });
    super::state_record::initialize_slots(
        stream.get_nanbox_f64(),
        &[
            (Slot::ReadableFlag, bool_value(true)),
            (Slot::ReadableLivePush, bool_value(true)),
            (Slot::ReadableAborted, bool_value(false)),
            (Slot::ReadableObjectMode, bool_value(mode)),
            (Slot::ReadableFlowing, f64::from_bits(TAG_NULL)),
            (Slot::Readable, bool_value(true)),
            (Slot::ReadableEnded, bool_value(false)),
            (Slot::EndEmitted, bool_value(false)),
            (Slot::Disturbed, bool_value(false)),
            (Slot::ReadableEncoding, encoding.get_nanbox_f64()),
            (Slot::EmitClose, bool_value(emit_close)),
            (Slot::AutoDestroy, bool_value(auto_destroy)),
        ],
        &[
            (StateData::ReadableBuffered, 0.0),
            (StateData::ReadableHwm, hwm),
            (StateData::Pipes, pipes.get_nanbox_f64()),
        ],
    );
    install_readable_state_view(stream.get_nanbox_f64());
}

/// Writable uses the same normalized-record birth and state-object allocator.
pub(super) fn init_writable_state(
    stream: &RuntimeHandle<'_>,
    opts: &RuntimeHandle<'_>,
    duplex: bool,
) {
    let mode = resolve_object_mode(
        opts,
        crate::runtime_state_key!(b"writableObjectMode"),
        duplex,
    );
    let hwm = normalized_hwm(
        opts,
        crate::runtime_state_key!(b"writableHighWaterMark"),
        "options.writableHighWaterMark",
        duplex,
        mode,
    );
    let decode = crate::runtime_state_key!(b"decodeStrings")
        .read_value(opts.get_nanbox_f64())
        .to_bits()
        != TAG_FALSE;
    let emit_close = crate::runtime_state_key!(b"emitClose")
        .read_value(opts.get_nanbox_f64())
        .to_bits()
        != TAG_FALSE;
    let auto_destroy = crate::runtime_state_key!(b"autoDestroy")
        .read_value(opts.get_nanbox_f64())
        .to_bits()
        != TAG_FALSE;
    let scope = crate::gc::RuntimeHandleScope::new();
    let encoding = scope.root_nanbox_f64(normalized_default_encoding(opts));
    let bool_value = |on| f64::from_bits(if on { TAG_TRUE } else { TAG_FALSE });
    super::state_record::initialize_slots(
        stream.get_nanbox_f64(),
        &[
            (Slot::WritableFlag, bool_value(true)),
            (Slot::WritableObjectMode, bool_value(mode)),
            (Slot::WritableDecodeStrings, bool_value(decode)),
            (Slot::WritableDefaultEncoding, encoding.get_nanbox_f64()),
            (Slot::WritableNeedDrain, bool_value(false)),
            (Slot::Writable, bool_value(true)),
            (Slot::WritableEnded, bool_value(false)),
            (Slot::WritableFinished, bool_value(false)),
            (Slot::EmitClose, bool_value(emit_close)),
            (Slot::AutoDestroy, bool_value(auto_destroy)),
            (
                Slot::WritablePendingFinishCallback,
                f64::from_bits(TAG_UNDEFINED),
            ),
        ],
        &[
            (StateData::WritableHwm, hwm),
            (StateData::WritableLength, 0.0),
            (StateData::WritableCorked, 0.0),
        ],
    );
    install_writable_state_view(stream.get_nanbox_f64());
}

pub(super) fn init_duplex_state(stream: &RuntimeHandle<'_>, opts: &RuntimeHandle<'_>) {
    let allow_half_open = if get_hidden_value(
        opts.get_nanbox_f64(),
        crate::runtime_state_key!(b"allowHalfOpen"),
    )
    .is_some_and(|v| v.to_bits() == TAG_FALSE)
    {
        TAG_FALSE
    } else {
        TAG_TRUE
    };
    // node's own enumerable `allowHalfOpen`.
    stream_birth_field!(
        stream.get_nanbox_f64(),
        crate::runtime_state_key!(b"allowHalfOpen"),
        f64::from_bits(allow_half_open),
    );

    if crate::runtime_state_key!(b"readable")
        .read_value(opts.get_nanbox_f64())
        .to_bits()
        == TAG_FALSE
    {
        let state =
            crate::runtime_state_key!(b"_readableState").read_value(stream.get_nanbox_f64());
        birth::ordinary_birth_set(
            state,
            crate::runtime_state_key!(b"readable"),
            f64::from_bits(TAG_FALSE),
        );
        super::state_record::initialize_slots(
            stream.get_nanbox_f64(),
            &[
                (Slot::Readable, f64::from_bits(TAG_FALSE)),
                (Slot::ReadableEnded, f64::from_bits(TAG_TRUE)),
                (Slot::EndEmitted, f64::from_bits(TAG_TRUE)),
            ],
            &[],
        );
    }
    if crate::runtime_state_key!(b"writable")
        .read_value(opts.get_nanbox_f64())
        .to_bits()
        == TAG_FALSE
    {
        super::state_record::initialize_slots(
            stream.get_nanbox_f64(),
            &[
                (Slot::WritableOption, f64::from_bits(TAG_FALSE)),
                (Slot::Writable, f64::from_bits(TAG_FALSE)),
                (Slot::WritableEnded, f64::from_bits(TAG_TRUE)),
                (Slot::WritableFinished, f64::from_bits(TAG_TRUE)),
                (Slot::FinishEmitted, f64::from_bits(TAG_TRUE)),
            ],
            &[],
        );
    }
}

pub(super) fn init_abort_signal_state(stream: &RuntimeHandle<'_>, opts: &RuntimeHandle<'_>) {
    if let Some(signal) =
        options_signal(opts.get_nanbox_f64()).filter(|s| crate::value::js_is_truthy(*s) != 0)
    {
        attach_abort_signal(signal, stream.get_nanbox_f64());
    }
}

// ─────────────────────────────────────────────────────────────────
// #1987: the body of this module is split into topical siblings to stay
// under the 2000-line file-size gate. The constants, hidden-key accessors,
// option-parsing helpers and state primitives above are shared with each
// sibling via `use super::*`. Items referenced through the parent module's
// `pub use constructors::*` glob are re-exported by name below.
// ─────────────────────────────────────────────────────────────────

#[path = "node_stream_constructors/birth.rs"]
pub(super) mod birth;
#[path = "node_stream_constructors/birth_memo.rs"]
pub(super) mod birth_memo;
use birth::stream_birth_field;

#[path = "node_stream_constructors/builders.rs"]
mod builders;
#[path = "node_stream_constructors/event_emitter_new.rs"]
mod event_emitter_new;
#[path = "node_stream_constructors/instance_birth.rs"]
pub(crate) mod instance_birth;
pub(crate) use event_emitter_new::{emitter_new, own_prototype_by_bag};
#[path = "node_stream_constructors/introspection.rs"]
mod introspection;
#[path = "node_stream_constructors/pipeline.rs"]
mod pipeline;
#[path = "node_stream_constructors/web_adapter.rs"]
mod web_adapter;

pub(crate) use builders::ensure_lazy_stream;
pub use builders::{
    init_transform_in_place, init_writable_payload_in_place, js_array_subclass_init,
    js_event_emitter_async_resource_subclass_init, js_event_emitter_subclass_init,
    js_node_stream_duplex_new, js_node_stream_duplex_subclass_init,
    js_node_stream_legacy_subclass_init, js_node_stream_passthrough_new,
    js_node_stream_passthrough_subclass_init, js_node_stream_readable_from,
    js_node_stream_readable_from_options, js_node_stream_readable_new,
    js_node_stream_readable_subclass_init, js_node_stream_transform_new,
    js_node_stream_transform_subclass_init, js_node_stream_writable_new,
    js_node_stream_writable_subclass_init,
};
pub use event_emitter_new::{
    event_emitter_constructor_value, event_emitter_prototype_value,
    js_event_emitter_async_resource_call, js_event_emitter_async_resource_object_new,
    js_event_emitter_object_new,
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
