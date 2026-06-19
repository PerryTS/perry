//! node:stream — constructors, init/option-parsing, and module-level FFI entry points (split out of node_stream.rs for the 2000-line
//! file-size gate, #1987). Shares the parent module's constants, hidden-key
//! accessors and state primitives via `use super::*`.
#![allow(unused_imports)]
use super::*;
use crate::closure::{
    js_closure_alloc, js_closure_get_capture_f64, js_closure_get_capture_ptr,
    js_closure_set_capture_f64, js_closure_set_capture_ptr, ClosureHeader,
};
use crate::object::{
    js_object_alloc, js_object_alloc_with_shape, js_object_get_field,
    js_object_get_field_by_name_f64, js_object_set_field, js_object_set_field_by_name,
    ObjectHeader,
};
use crate::value::JSValue;
use std::os::raw::c_int;
use std::sync::atomic::{AtomicPtr, Ordering};

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

#[no_mangle]
pub extern "C" fn js_node_stream_readable_new(opts: f64) -> f64 {
    register_iter_helper_arities();
    let methods = readable_methods();
    let obj = build_object(&methods, READABLE_SHAPE_ID + methods.len() as u32);
    let readable = f64::from_bits(JSValue::pointer(obj as *const u8).bits());
    if let Some(read) = read_callback_from_options(opts) {
        js_object_set_field_by_name(obj, hidden_read_key(), rebind_callback_this(read, readable));
    } else {
        set_hidden_value(
            readable,
            hidden_default_read_error_key(),
            f64::from_bits(TAG_TRUE),
        );
    }
    init_lifecycle_state(readable, opts);
    init_constructor(readable, "Readable");
    init_readable_state(readable, opts);
    install_common_lifecycle_callbacks(readable, opts);
    init_abort_signal_state(readable, opts);
    async_iterator::install_readable_async_iterator_symbol(readable);
    install_stream_async_dispose_symbol(readable);
    invoke_construct_callback(readable, opts);
    readable
}

#[no_mangle]
pub extern "C" fn js_node_stream_readable_subclass_init(this: f64, opts: f64) -> f64 {
    register_iter_helper_arities();
    let raw = raw_ptr_from_value(this);
    if raw == 0 {
        return this;
    }
    if unsafe { gc_type_for_ptr(raw) } != Some(crate::gc::GC_TYPE_OBJECT) {
        return this;
    }

    let obj = raw as *mut ObjectHeader;
    let subclass_read =
        js_object_get_field_by_name_f64(obj as *const ObjectHeader, hidden_key(b"_read"));

    let methods = readable_methods();
    install_methods_on_existing_object(obj, this, &methods, &[]);

    if let Some(read) = read_callback_from_options(opts) {
        js_object_set_field_by_name(obj, hidden_read_key(), rebind_callback_this(read, this));
    } else if is_callable_value(subclass_read) {
        js_object_set_field_by_name(obj, hidden_read_key(), subclass_read);
    }

    init_lifecycle_state(this, opts);
    init_constructor(this, "Readable");
    init_readable_state(this, opts);
    install_common_lifecycle_callbacks(this, opts);
    init_abort_signal_state(this, opts);
    async_iterator::install_readable_async_iterator_symbol(this);
    install_stream_async_dispose_symbol(this);
    invoke_construct_callback(this, opts);
    this
}

/// #5137: `super()` for a source-compiled `class X extends EventEmitter`
/// (from `node:events`). Installs the bare EventEmitter listener/emit
/// methods directly onto `this` — the same generic `ns_*` closures the
/// stream subclasses use — so `.on`/`.emit`/`.once`/… resolve as the
/// instance's own bound methods. This is the EventEmitter analog of
/// `js_node_stream_readable_subclass_init`; commander's `Command extends
/// EventEmitter` reaches it when its real npm source is compiled (the
/// package is in `perry.compilePackages`, so the `new Command()` → native
/// `js_commander_*` shim path is deliberately off). Unlike the stream
/// inits there is no option-driven state to seed — a plain EventEmitter
/// has no `_read`/`highWaterMark`/etc.
#[no_mangle]
pub extern "C" fn js_event_emitter_subclass_init(this: f64) -> f64 {
    let raw = raw_ptr_from_value(this);
    if raw == 0 {
        return this;
    }
    if unsafe { gc_type_for_ptr(raw) } != Some(crate::gc::GC_TYPE_OBJECT) {
        return this;
    }
    let obj = raw as *mut ObjectHeader;
    let methods = emitter_methods();
    install_methods_on_existing_object(obj, this, &methods, &[]);
    this
}

#[no_mangle]
pub extern "C" fn js_node_stream_writable_new(opts: f64) -> f64 {
    let methods = writable_methods();
    let obj = build_object(&methods, WRITABLE_SHAPE_ID + methods.len() as u32);
    let writable = f64::from_bits(JSValue::pointer(obj as *const u8).bits());
    if let Some(write) = write_callback_from_options(opts) {
        js_object_set_field_by_name(
            obj,
            hidden_write_key(),
            rebind_callback_this(write, writable),
        );
    }
    if let Some(writev) = writev_callback_from_options(opts) {
        js_object_set_field_by_name(
            obj,
            hidden_writev_key(),
            rebind_callback_this(writev, writable),
        );
    }
    init_lifecycle_state(writable, opts);
    init_constructor(writable, "Writable");
    init_writable_state(writable, opts);
    install_common_lifecycle_callbacks(writable, opts);
    install_writable_lifecycle_callbacks(writable, opts);
    init_abort_signal_state(writable, opts);
    install_stream_async_dispose_symbol(writable);
    invoke_construct_callback(writable, opts);
    writable
}

#[no_mangle]
pub extern "C" fn js_node_stream_writable_subclass_init(this: f64, opts: f64) -> f64 {
    let obj = {
        let bits = this.to_bits();
        let top16 = bits >> 48;
        let raw = if top16 >= 0x7FF8 {
            if top16 == 0x7FFC {
                return f64::from_bits(TAG_UNDEFINED);
            }
            (bits & crate::value::POINTER_MASK) as usize
        } else {
            bits as usize
        };
        if raw < crate::gc::GC_HEADER_SIZE + 0x1000 {
            return f64::from_bits(TAG_UNDEFINED);
        }
        raw as *mut ObjectHeader
    };
    let this = f64::from_bits(JSValue::pointer(obj as *const u8).bits());
    unsafe {
        if gc_type_for_ptr(obj as usize) != Some(crate::gc::GC_TYPE_OBJECT) {
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    if obj.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }

    let subclass_write = js_object_get_field_by_name_f64(obj, hidden_key(b"_write"));
    let subclass_writev = js_object_get_field_by_name_f64(obj, hidden_key(b"_writev"));
    let methods = writable_methods();
    install_methods_on_existing_object(obj, this, &methods, &["_write"]);

    if let Some(write) = write_callback_from_options(opts) {
        js_object_set_field_by_name(obj, hidden_write_key(), rebind_callback_this(write, this));
    } else if is_callable_value(subclass_write) {
        js_object_set_field_by_name(obj, hidden_write_key(), subclass_write);
    }
    if let Some(writev) = writev_callback_from_options(opts) {
        js_object_set_field_by_name(obj, hidden_writev_key(), rebind_callback_this(writev, this));
    } else if is_callable_value(subclass_writev) {
        js_object_set_field_by_name(obj, hidden_writev_key(), subclass_writev);
    }

    init_lifecycle_state(this, opts);
    init_constructor(this, "Writable");
    init_writable_state(this, opts);
    install_common_lifecycle_callbacks(this, opts);
    install_writable_lifecycle_callbacks(this, opts);
    init_abort_signal_state(this, opts);
    install_stream_async_dispose_symbol(this);
    invoke_construct_callback(this, opts);
    this
}

#[no_mangle]
pub extern "C" fn js_node_stream_duplex_new(opts: f64) -> f64 {
    register_iter_helper_arities();
    let methods = duplex_methods();
    let obj = build_object(&methods, DUPLEX_SHAPE_ID + methods.len() as u32);
    let duplex = f64::from_bits(JSValue::pointer(obj as *const u8).bits());
    if let Some(read) = read_callback_from_options(opts) {
        js_object_set_field_by_name(obj, hidden_read_key(), rebind_callback_this(read, duplex));
    }
    if let Some(write) = write_callback_from_options(opts) {
        js_object_set_field_by_name(obj, hidden_write_key(), rebind_callback_this(write, duplex));
        set_hidden_value(
            duplex,
            hidden_key(b"writableCustomSink"),
            f64::from_bits(TAG_TRUE),
        );
    }
    if let Some(writev) = writev_callback_from_options(opts) {
        js_object_set_field_by_name(
            obj,
            hidden_writev_key(),
            rebind_callback_this(writev, duplex),
        );
        set_hidden_value(
            duplex,
            hidden_key(b"writableCustomSink"),
            f64::from_bits(TAG_TRUE),
        );
    }
    init_lifecycle_state(duplex, opts);
    init_constructor(duplex, "Duplex");
    init_readable_state(duplex, opts);
    init_writable_state(duplex, opts);
    init_duplex_state(duplex, opts);
    install_common_lifecycle_callbacks(duplex, opts);
    install_writable_lifecycle_callbacks(duplex, opts);
    init_abort_signal_state(duplex, opts);
    async_iterator::install_readable_async_iterator_symbol(duplex);
    install_stream_async_dispose_symbol(duplex);
    invoke_construct_callback(duplex, opts);
    duplex
}

#[no_mangle]
pub extern "C" fn js_node_stream_duplex_subclass_init(this: f64, opts: f64) -> f64 {
    register_iter_helper_arities();
    let raw = raw_ptr_from_value(this);
    if raw == 0 {
        return this;
    }
    if unsafe { gc_type_for_ptr(raw) } != Some(crate::gc::GC_TYPE_OBJECT) {
        return this;
    }

    let obj = raw as *mut ObjectHeader;
    let subclass_read =
        js_object_get_field_by_name_f64(obj as *const ObjectHeader, hidden_key(b"_read"));
    let subclass_write = js_object_get_field_by_name_f64(obj, hidden_key(b"_write"));
    let subclass_writev = js_object_get_field_by_name_f64(obj, hidden_key(b"_writev"));

    let methods = duplex_methods();
    install_methods_on_existing_object(obj, this, &methods, &[]);

    if let Some(read) = read_callback_from_options(opts) {
        js_object_set_field_by_name(obj, hidden_read_key(), rebind_callback_this(read, this));
    } else if is_callable_value(subclass_read) {
        js_object_set_field_by_name(obj, hidden_read_key(), subclass_read);
    }
    if let Some(write) = write_callback_from_options(opts) {
        js_object_set_field_by_name(obj, hidden_write_key(), rebind_callback_this(write, this));
        set_hidden_value(
            this,
            hidden_key(b"writableCustomSink"),
            f64::from_bits(TAG_TRUE),
        );
    } else if is_callable_value(subclass_write) {
        js_object_set_field_by_name(obj, hidden_write_key(), subclass_write);
        set_hidden_value(
            this,
            hidden_key(b"writableCustomSink"),
            f64::from_bits(TAG_TRUE),
        );
    }
    if let Some(writev) = writev_callback_from_options(opts) {
        js_object_set_field_by_name(obj, hidden_writev_key(), rebind_callback_this(writev, this));
        set_hidden_value(
            this,
            hidden_key(b"writableCustomSink"),
            f64::from_bits(TAG_TRUE),
        );
    } else if is_callable_value(subclass_writev) {
        js_object_set_field_by_name(obj, hidden_writev_key(), subclass_writev);
        set_hidden_value(
            this,
            hidden_key(b"writableCustomSink"),
            f64::from_bits(TAG_TRUE),
        );
    }

    init_lifecycle_state(this, opts);
    init_constructor(this, "Duplex");
    init_readable_state(this, opts);
    init_writable_state(this, opts);
    init_duplex_state(this, opts);
    install_common_lifecycle_callbacks(this, opts);
    install_writable_lifecycle_callbacks(this, opts);
    init_abort_signal_state(this, opts);
    async_iterator::install_readable_async_iterator_symbol(this);
    install_stream_async_dispose_symbol(this);
    invoke_construct_callback(this, opts);
    this
}

#[no_mangle]
pub extern "C" fn js_node_stream_transform_new(opts: f64) -> f64 {
    let transform = js_node_stream_duplex_new(opts);
    if let Some(callback) = transform_callback_from_options(opts) {
        set_hidden_value(
            transform,
            hidden_transform_callback_key(),
            rebind_callback_this(callback, transform),
        );
    }
    if let Some(flush) = transform_flush_from_options(opts) {
        set_hidden_value(
            transform,
            hidden_transform_flush_key(),
            rebind_callback_this(flush, transform),
        );
    }
    init_constructor(transform, "Transform");
    transform
}

#[no_mangle]
pub extern "C" fn js_node_stream_transform_subclass_init(this: f64, opts: f64) -> f64 {
    let transform = js_node_stream_duplex_subclass_init(this, opts);
    let raw = raw_ptr_from_value(transform);
    if raw == 0 {
        return transform;
    }
    if unsafe { gc_type_for_ptr(raw) } != Some(crate::gc::GC_TYPE_OBJECT) {
        return transform;
    }

    let obj = raw as *mut ObjectHeader;
    let subclass_transform = js_object_get_field_by_name_f64(obj, hidden_key(b"_transform"));
    let subclass_flush = js_object_get_field_by_name_f64(obj, hidden_key(b"_flush"));

    if let Some(callback) = transform_callback_from_options(opts) {
        set_hidden_value(
            transform,
            hidden_transform_callback_key(),
            rebind_callback_this(callback, transform),
        );
    } else if is_callable_value(subclass_transform) {
        set_hidden_value(
            transform,
            hidden_transform_callback_key(),
            subclass_transform,
        );
    }
    if let Some(flush) = transform_flush_from_options(opts) {
        set_hidden_value(
            transform,
            hidden_transform_flush_key(),
            rebind_callback_this(flush, transform),
        );
    } else if is_callable_value(subclass_flush) {
        set_hidden_value(transform, hidden_transform_flush_key(), subclass_flush);
    }
    init_constructor(transform, "Transform");
    transform
}

#[no_mangle]
pub extern "C" fn js_node_stream_passthrough_new(opts: f64) -> f64 {
    let passthrough = js_node_stream_duplex_new(opts);
    set_hidden_value(
        passthrough,
        hidden_transform_passthrough_key(),
        f64::from_bits(TAG_TRUE),
    );
    init_constructor(passthrough, "PassThrough");
    passthrough
}

/// `Readable.from(iterable)` — Node's static factory. Returns a
/// Readable object and retains simple iterable chunks so
/// `node:stream/consumers` can drain the current stub stream surface.
#[no_mangle]
pub extern "C" fn js_node_stream_readable_from(iterable: f64) -> f64 {
    js_node_stream_readable_from_options(iterable, f64::from_bits(TAG_UNDEFINED))
}

#[no_mangle]
pub extern "C" fn js_node_stream_readable_from_options(iterable: f64, opts: f64) -> f64 {
    if matches!(iterable.to_bits(), TAG_NULL | TAG_UNDEFINED)
        || is_non_iterable_primitive_for_readable_from(iterable)
    {
        throw_readable_from_invalid_iterable();
    }
    let readable = js_node_stream_readable_new(readable_from_options(opts));
    let raw = raw_ptr_from_value(readable);
    if raw >= 0x10000 {
        let trap_buf = crate::exception::js_try_push();
        let jumped = unsafe { crate::ffi::setjmp::setjmp(trap_buf as *mut c_int) };
        if jumped == 0 {
            let normalized = normalize_readable_from_input(iterable);
            crate::exception::js_try_end();
            js_object_set_field_by_name(
                raw as *mut ObjectHeader,
                hidden_chunks_key(),
                normalized.chunks,
            );
            initialize_readable_from_buffered_length(readable, normalized.chunks);
            if let Some(source_iterator) = normalized.source_iterator {
                js_object_set_field_by_name(
                    raw as *mut ObjectHeader,
                    hidden_key(READABLE_SOURCE_ITERATOR_KEY),
                    source_iterator,
                );
            }
        } else {
            let err = crate::exception::js_get_exception();
            crate::exception::js_clear_exception();
            crate::exception::js_try_end();
            destroy_stream(readable, err);
        }
    }
    readable
}

fn initialize_readable_from_buffered_length(readable: f64, chunks: f64) {
    let mut values = Vec::new();
    push_chunk_values(chunks, &mut values, 0);
    let length = if readable_object_mode(readable) {
        values.len() as f64
    } else {
        let mut bytes = Vec::new();
        for value in values {
            append_chunk_bytes(value, &mut bytes, 0);
        }
        bytes.len() as f64
    };
    set_hidden_value(readable, hidden_buffered_key(), length);
    set_hidden_value(readable, hidden_key(b"readableLength"), length);
}

// ─────────────────────────────────────────────────────────────────
// #1534: static introspection helpers `Readable.isDisturbed(s)` and
// `Readable.isErrored(s)`. Node returns booleans reflecting the
// stream's internal state machine; Perry's stream stubs don't track
// any of that state yet, so both return `false` — which is the
// correct answer for a freshly-constructed, untouched stream. The
// directional helpers `isReadable` / `isWritable` aren't here
// because Node's answer depends on the stream's actual direction
// (Readable returns `true` for isReadable + `null` for isWritable
// and so on); a uniform stub would lie for at least one case, so
// they're deferred until Perry's stream stub tracks direction.
// ─────────────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn js_node_stream_is_disturbed(stream: f64) -> f64 {
    if get_hidden_value(stream, hidden_disturbed_key())
        .is_some_and(|v| crate::value::js_is_truthy(v) != 0)
    {
        f64::from_bits(TAG_TRUE)
    } else {
        f64::from_bits(TAG_FALSE)
    }
}

#[no_mangle]
pub extern "C" fn js_node_stream_is_errored(stream: f64) -> f64 {
    if readable_hidden_error(stream).is_some() {
        f64::from_bits(TAG_TRUE)
    } else {
        f64::from_bits(TAG_FALSE)
    }
}

/// #1534/#1746: `Readable.isReadable(s)` / module-level `isReadable(s)`.
/// Node returns `null` for a stream with no readable side (e.g. a bare
/// Writable), `false` once the readable side has ended or errored, and
/// `true` while it's still readable. Perry tracks the readable-direction
/// flag at construction and the ended/errored bits as methods run.
#[no_mangle]
pub extern "C" fn js_node_stream_is_readable(stream: f64) -> f64 {
    if get_hidden_value(stream, hidden_readable_flag_key()).is_none() {
        return f64::from_bits(TAG_NULL);
    }
    let ended = stream_hidden_ended(stream);
    let errored = readable_hidden_error(stream).is_some();
    if ended || errored {
        f64::from_bits(TAG_FALSE)
    } else {
        f64::from_bits(TAG_TRUE)
    }
}

/// #1746: `stream.isWritable(s)` / `Writable.isWritable(s)`. Mirror of
/// `isReadable` for the writable side: `null` for a stream with no
/// writable side (a bare Readable), `false` once it has ended (`.end()`)
/// or errored, `true` otherwise. A Duplex answers for its writable side.
#[no_mangle]
pub extern "C" fn js_node_stream_is_writable(stream: f64) -> f64 {
    if get_hidden_value(stream, hidden_writable_flag_key()).is_none() {
        return f64::from_bits(TAG_NULL);
    }
    let ended = stream_hidden_ended(stream);
    let errored = readable_hidden_error(stream).is_some();
    if ended || errored {
        f64::from_bits(TAG_FALSE)
    } else {
        f64::from_bits(TAG_TRUE)
    }
}

/// #2685: `stream.isDestroyed(s)`. Node returns `null` for non-streams and a
/// boolean for real stream instances.
#[no_mangle]
pub extern "C" fn js_node_stream_is_destroyed(stream: f64) -> f64 {
    if !is_classic_stream_instance_value(stream) {
        return f64::from_bits(TAG_NULL);
    }
    f64::from_bits(if stream_destroyed(stream) {
        TAG_TRUE
    } else {
        TAG_FALSE
    })
}

fn bool_value(value: bool) -> f64 {
    f64::from_bits(if value { TAG_TRUE } else { TAG_FALSE })
}

fn stream_value_addr(value: f64) -> Option<usize> {
    let jsv = JSValue::from_bits(value.to_bits());
    if !jsv.is_pointer() {
        return None;
    }
    let addr = (value.to_bits() & crate::value::POINTER_MASK) as usize;
    if addr < 0x10000 {
        None
    } else {
        Some(addr)
    }
}

/// #2685: `stream._isArrayBufferView(value)` aliases Node's stream-local
/// helper semantics, where Buffer counts as an ArrayBuffer view.
#[no_mangle]
pub extern "C" fn js_node_stream_is_array_buffer_view(value: f64) -> f64 {
    let Some(addr) = stream_value_addr(value) else {
        return f64::from_bits(TAG_FALSE);
    };
    let registered_view = crate::buffer::is_registered_buffer(addr)
        && (!crate::buffer::is_any_array_buffer(addr)
            || crate::buffer::is_uint8array_buffer(addr)
            || crate::buffer::is_data_view(addr));
    bool_value(registered_view || crate::typedarray::lookup_typed_array_kind(addr).is_some())
}

/// #2685: `stream._isUint8Array(value)` returns true for Buffer as well as
/// Uint8Array instances, matching Node's internal type predicate.
#[no_mangle]
pub extern "C" fn js_node_stream_is_uint8_array(value: f64) -> f64 {
    let Some(addr) = stream_value_addr(value) else {
        return f64::from_bits(TAG_FALSE);
    };
    let registered_uint8 = crate::buffer::is_registered_buffer(addr)
        && (crate::buffer::is_uint8array_buffer(addr)
            || (!crate::buffer::is_any_array_buffer(addr) && !crate::buffer::is_data_view(addr)));
    bool_value(
        registered_uint8
            || crate::typedarray::lookup_typed_array_kind(addr)
                == Some(crate::typedarray::KIND_UINT8),
    )
}

fn stream_byte_view_bytes(value: f64) -> Vec<u8> {
    let Some(addr) = stream_value_addr(value) else {
        return Vec::new();
    };
    if crate::buffer::is_any_array_buffer(addr)
        && !crate::buffer::is_uint8array_buffer(addr)
        && !crate::buffer::is_data_view(addr)
    {
        return Vec::new();
    }
    if crate::buffer::is_registered_buffer(addr) {
        let data = crate::buffer::js_native_buffer_data_ptr(value);
        let len = crate::buffer::js_native_buffer_byte_len(value);
        if data.is_null() || len == 0 {
            return Vec::new();
        }
        return unsafe { std::slice::from_raw_parts(data, len).to_vec() };
    }
    if crate::typedarray::lookup_typed_array_kind(addr).is_some() {
        let ta = addr as *const crate::typedarray::TypedArrayHeader;
        return unsafe {
            crate::typedarray::typed_array_bytes(ta)
                .map(|bytes| bytes.to_vec())
                .unwrap_or_default()
        };
    }
    Vec::new()
}

/// #2685: `stream._uint8ArrayToBuffer(view)` returns a Buffer containing the
/// bytes visible through the passed ArrayBuffer view.
#[no_mangle]
pub extern "C" fn js_node_stream_uint8_array_to_buffer(value: f64) -> f64 {
    buffer_value_from_bytes(&stream_byte_view_bytes(value))
}

/// #1537: `stream.getDefaultHighWaterMark(objectMode)` returns the current
/// platform-default highWaterMark — 65536 for byte streams, 16 for
/// objectMode (both settable via `setDefaultHighWaterMark`).
#[no_mangle]
pub extern "C" fn js_node_stream_get_default_hwm(object_mode: f64) -> f64 {
    default_hwm(crate::value::js_is_truthy(object_mode) != 0)
}

/// #1537: `stream.setDefaultHighWaterMark(objectMode, value)` updates the
/// per-mode default returned by `getDefaultHighWaterMark` and inherited by
/// streams constructed without an explicit `highWaterMark`. Returns
/// `undefined`, matching Node.
#[no_mangle]
pub extern "C" fn js_node_stream_set_default_hwm(object_mode: f64, value: f64) -> f64 {
    let n = jsvalue_as_f64(value).unwrap_or(0.0);
    if crate::value::js_is_truthy(object_mode) != 0 {
        DEFAULT_HWM_OBJECT.with(|c| c.set(n));
    } else {
        DEFAULT_HWM_BYTE.with(|c| c.set(n));
    }
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) fn attach_abort_signal(signal: f64, stream: f64) {
    if signal_is_aborted(signal) {
        destroy_stream(stream, abort_error());
        return;
    }
    let Some(signal_obj) = object_ptr_from_value(signal) else {
        return;
    };
    let listener = js_closure_alloc(ns_stream_abort_listener as *const u8, 1);
    js_closure_set_capture_ptr(listener, 0, stream.to_bits() as i64);
    crate::url::js_abort_signal_add_listener(
        signal_obj,
        string_value(b"abort"),
        box_pointer(listener as *const u8),
    );
}

/// #1541: `stream.addAbortSignal(signal, stream)` — wire an AbortSignal so
/// aborting it destroys the stream with an AbortError, then return the same
/// stream for chaining.
#[no_mangle]
pub extern "C" fn js_node_stream_add_abort_signal(signal: f64, stream: f64) -> f64 {
    attach_abort_signal(signal, stream);
    stream
}

fn attach_duplex_readable_source(duplex: f64, source: f64) -> Result<(), f64> {
    let chunks = if let Some(chunks) = readable_hidden_chunks(source) {
        chunks
    } else {
        collect_pipeline_chunks(source)?
    };
    let values = pipeline_chunks_vec(chunks);
    let mut arr = crate::array::js_array_alloc(values.len() as u32);
    for chunk in values {
        arr = crate::array::js_array_push_f64(arr, chunk);
    }

    set_hidden_value(duplex, hidden_chunks_key(), box_pointer(arr as *const u8));
    set_hidden_value(
        duplex,
        hidden_buffered_key(),
        crate::array::js_array_length(arr) as f64,
    );
    set_hidden_value(
        duplex,
        hidden_key(b"readableLength"),
        crate::array::js_array_length(arr) as f64,
    );
    Ok(())
}

fn node_stream_duplex_from_source_chunks(source: f64) -> f64 {
    let duplex = js_node_stream_duplex_new(readable_from_options(f64::from_bits(TAG_UNDEFINED)));
    set_visible_writable(duplex, false);
    if let Err(err) = attach_duplex_readable_source(duplex, source) {
        set_hidden_value(duplex, hidden_error_key(), err);
    }
    duplex
}

pub(super) extern "C" fn duplex_from_writable_write_callback(
    closure: *const ClosureHeader,
    chunk: f64,
    encoding: f64,
    cb: f64,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let writable = js_closure_get_capture_f64(closure, 0);
    js_node_stream_method_write(raw_ptr_from_value(writable) as i64, chunk, encoding, cb)
}

pub(super) extern "C" fn duplex_from_writable_final_callback(
    closure: *const ClosureHeader,
    cb: f64,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let writable = js_closure_get_capture_f64(closure, 0);
    js_node_stream_method_end(
        raw_ptr_from_value(writable) as i64,
        f64::from_bits(TAG_UNDEFINED),
    );
    call_listener_args(writable, cb, &[]);
    f64::from_bits(TAG_UNDEFINED)
}

fn install_duplex_from_writable(duplex: f64, writable: f64) {
    let raw = raw_ptr_from_value(duplex);
    if raw < 0x10000 {
        return;
    }
    let obj = raw as *mut ObjectHeader;
    let write = js_closure_alloc(duplex_from_writable_write_callback as *const u8, 1);
    js_closure_set_capture_f64(write, 0, writable);
    js_object_set_field_by_name(
        obj,
        hidden_write_key(),
        f64::from_bits(JSValue::pointer(write as *const u8).bits()),
    );

    let final_cb = js_closure_alloc(duplex_from_writable_final_callback as *const u8, 1);
    js_closure_set_capture_f64(final_cb, 0, writable);
    js_object_set_field_by_name(
        obj,
        hidden_writable_final_key(),
        f64::from_bits(JSValue::pointer(final_cb as *const u8).bits()),
    );

    set_hidden_value(duplex, hidden_key(b"duplexWrappedWritable"), writable);
    set_hidden_value(
        duplex,
        hidden_key(b"writableCustomSink"),
        f64::from_bits(TAG_TRUE),
    );
}

#[no_mangle]
pub extern "C" fn js_node_stream_duplex_from_options(body: f64, _opts: f64) -> f64 {
    if object_ptr_from_value(body).is_some() && !is_classic_stream_instance_value(body) {
        let readable = get_hidden_value(body, hidden_key(b"readable"));
        let writable = get_hidden_value(body, hidden_key(b"writable"));
        if readable.is_some() || writable.is_some() {
            let duplex =
                js_node_stream_duplex_new(readable_from_options(f64::from_bits(TAG_UNDEFINED)));
            if let Some(readable) = readable {
                if let Err(err) = attach_duplex_readable_source(duplex, readable) {
                    set_hidden_value(duplex, hidden_error_key(), err);
                }
            } else {
                set_visible_readable(duplex, false);
            }
            if let Some(writable) = writable {
                install_duplex_from_writable(duplex, writable);
            } else {
                set_visible_writable(duplex, false);
            }
            return duplex;
        }
    }

    node_stream_duplex_from_source_chunks(body)
}

/// #1539: `stream.compose(...streams)` chains a sequence of streams or
/// callable stages into one composite Duplex.
#[no_mangle]
pub extern "C" fn js_node_stream_compose(args: *const crate::array::ArrayHeader) -> f64 {
    js_node_stream_compose_args(args)
}

/// Variadic `stream.compose(...)` entry used by bound native-module property
/// reads and by direct named imports through codegen's packed varargs ABI.
pub extern "C" fn js_node_stream_compose_args(args: *const crate::array::ArrayHeader) -> f64 {
    build_node_stream_compose(pipeline_args(args))
}

pub(super) fn add_finished_once_listeners(
    stream: f64,
    callback: f64,
    watch_finish: bool,
    watch_close: bool,
) {
    let listener = js_closure_alloc(ns_finished_error_false_close as *const u8, 3);
    js_closure_set_capture_f64(listener, 0, stream);
    js_closure_set_capture_f64(listener, 1, callback);
    js_closure_set_capture_f64(listener, 2, f64::from_bits(TAG_FALSE));
    let listener_value = box_pointer(listener as *const u8);
    if watch_finish {
        add_stream_listener_for_event(stream, string_value(b"finish"), listener_value);
    }
    if watch_close {
        add_stream_listener_for_event(stream, string_value(b"close"), listener_value);
    }
}

pub(super) fn add_finished_signal_abort_listener(stream: f64, signal: f64, callback: f64) {
    let listener = js_closure_alloc(ns_finished_signal_abort as *const u8, 4);
    js_closure_set_capture_f64(listener, 0, stream);
    js_closure_set_capture_f64(listener, 1, callback);
    js_closure_set_capture_f64(listener, 2, f64::from_bits(TAG_FALSE));
    js_closure_set_capture_f64(listener, 3, signal);
    if signal_is_aborted(signal) {
        crate::builtins::js_queue_microtask(listener as i64);
        return;
    }
    let Some(signal_obj) = object_ptr_from_value(signal) else {
        return;
    };
    crate::url::js_abort_signal_add_listener(
        signal_obj,
        string_value(b"abort"),
        box_pointer(listener as *const u8),
    );
}

pub(super) fn add_finished_cleanup_completion_listener(stream: f64, callback: f64) {
    let listener = js_closure_alloc(ns_finished_error_false_close as *const u8, 3);
    js_closure_set_capture_f64(listener, 0, stream);
    js_closure_set_capture_f64(listener, 1, callback);
    js_closure_set_capture_f64(listener, 2, f64::from_bits(TAG_FALSE));
    let listener_value = box_pointer(listener as *const u8);
    add_stream_listener_for_event(stream, string_value(b"end"), listener_value);
    add_stream_listener_for_event(stream, string_value(b"finish"), listener_value);
    add_stream_listener_for_event(stream, string_value(b"close"), listener_value);
}

/// `stream.finished(stream, [options], cb)` callback form. This slice covers
/// focused option paths:
///
/// - `{ error: false }`: do not install an error listener, but `close` still
///   observes the stream's stored error and calls the callback.
/// - `{ readable: false }`: ignore the readable side and call back when the
///   writable side emits `finish`.
#[no_mangle]
pub extern "C" fn js_node_stream_finished(args: *const crate::array::ArrayHeader) -> f64 {
    let args = pipeline_args(args);
    if args.len() < 2 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stream = args[0];
    let mut options = f64::from_bits(TAG_UNDEFINED);
    let mut callback = args[1];
    if args.len() >= 3 && is_pipeline_options_arg(args[1]) {
        options = args[1];
        callback = args[2];
    }
    if !is_callable_value(callback) {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let watch_close =
        get_hidden_value(options, hidden_key(b"error")).is_some_and(|v| v.to_bits() == TAG_FALSE);
    let watch_finish = get_hidden_value(options, hidden_key(b"readable"))
        .is_some_and(|v| v.to_bits() == TAG_FALSE);
    if watch_close || watch_finish {
        add_finished_once_listeners(stream, callback, watch_finish, watch_close);
    }
    if let Some(signal) = options_signal(options) {
        add_finished_signal_abort_listener(stream, signal, callback);
    }
    if get_hidden_value(options, hidden_key(b"cleanup"))
        .is_some_and(|v| crate::value::js_is_truthy(v) != 0)
    {
        add_finished_cleanup_completion_listener(stream, callback);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `stream.pipeline(...streams, cb)` wires classic streams end-to-end and
/// invokes the callback once on success or on the first observed error.
#[no_mangle]
pub extern "C" fn js_node_stream_pipeline(args: *const crate::array::ArrayHeader) -> f64 {
    let mut args = pipeline_args(args);
    if args.is_empty() {
        throw_pipeline_missing_streams();
    }

    let callback = *args.last().unwrap_or(&f64::from_bits(TAG_UNDEFINED));
    if !is_callable_value(callback) {
        throw_pipeline_callback_required();
    }
    args.pop();

    let mut options = PipelineOptions {
        end_final: true,
        signal: None,
    };
    if args.last().copied().is_some_and(is_pipeline_options_arg) {
        let option_arg = args.pop().unwrap_or(f64::from_bits(TAG_UNDEFINED));
        options = pipeline_options_from_arg(option_arg);
    }

    if args.len() == 1 && is_array_like_value(args[0]) {
        args = pipeline_array_like_values(args[0]);
    }
    if args.len() < 2 {
        throw_pipeline_missing_streams();
    }

    if pipeline_needs_collected_path(&args) {
        return run_collected_pipeline(&args, callback, options);
    }

    let stages: Vec<f64> = args
        .into_iter()
        .enumerate()
        .map(|(idx, stage)| normalize_pipeline_source(stage, idx))
        .collect();
    add_pipeline_callback_listeners(&stages, callback, options);

    for i in 0..stages.len() - 1 {
        let is_final_pair = i + 1 == stages.len() - 1;
        wire_pipeline_pair(
            stages[i],
            stages[i + 1],
            options.end_final || !is_final_pair,
        );
    }
    for stage in stages.iter().take(stages.len() - 1) {
        start_pipeline_readable(*stage);
    }

    *stages.last().unwrap_or(&f64::from_bits(TAG_UNDEFINED))
}

pub(super) extern "C" fn duplex_pair_write_callback(
    closure: *const ClosureHeader,
    chunk: f64,
    _encoding: f64,
    cb: f64,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let peer = js_closure_get_capture_f64(closure, 0);
    if get_hidden_value(peer, hidden_readable_flag_key()).is_some() && !stream_destroyed(peer) {
        mark_disturbed(peer);
        if readable_is_flowing(peer) {
            emit_readable_data(peer, chunk);
        } else {
            buffer_pending_readable_chunk(peer, chunk);
        }
    }
    call_listener_args(peer, cb, &[]);
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) extern "C" fn duplex_pair_final_callback(closure: *const ClosureHeader, cb: f64) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let peer = js_closure_get_capture_f64(closure, 0);
    schedule_readable_end(peer);
    call_listener_args(peer, cb, &[]);
    f64::from_bits(TAG_UNDEFINED)
}

fn install_duplex_pair_endpoint(endpoint: f64, peer: f64) {
    let raw = raw_ptr_from_value(endpoint);
    if raw < 0x10000 {
        return;
    }
    let obj = raw as *mut ObjectHeader;
    let write = js_closure_alloc(duplex_pair_write_callback as *const u8, 1);
    js_closure_set_capture_f64(write, 0, peer);
    js_object_set_field_by_name(
        obj,
        hidden_write_key(),
        f64::from_bits(JSValue::pointer(write as *const u8).bits()),
    );

    let final_cb = js_closure_alloc(duplex_pair_final_callback as *const u8, 1);
    js_closure_set_capture_f64(final_cb, 0, peer);
    js_object_set_field_by_name(
        obj,
        hidden_writable_final_key(),
        f64::from_bits(JSValue::pointer(final_cb as *const u8).bits()),
    );

    set_hidden_value(endpoint, hidden_key(b"duplexPairPeer"), peer);
    set_hidden_value(
        endpoint,
        hidden_key(b"writableCustomSink"),
        f64::from_bits(TAG_TRUE),
    );
}

/// #1539: `stream.duplexPair([options])` returns a two-element array
/// `[Duplex, Duplex]` where writes to one show up as reads on the
/// other and vice versa.
#[no_mangle]
pub extern "C" fn js_node_stream_duplex_pair(_opts: f64) -> f64 {
    let a = js_node_stream_duplex_new(f64::from_bits(TAG_UNDEFINED));
    let b = js_node_stream_duplex_new(f64::from_bits(TAG_UNDEFINED));
    install_duplex_pair_endpoint(a, b);
    install_duplex_pair_endpoint(b, a);
    let arr = crate::array::js_array_alloc(2);
    crate::array::js_array_push(arr, JSValue::from_bits(a.to_bits()));
    crate::array::js_array_push(arr, JSValue::from_bits(b.to_bits()));
    f64::from_bits(JSValue::pointer(arr as *const u8).bits())
}

// ─────────────────────────────────────────────────────────────────
// #2521: Web-stream interop. Node exposes static helpers on the
// stream classes for converting between Node streams and WHATWG streams.
// The Web Streams implementation lives in perry-stdlib and registers the
// compact constructor/reader/writer callbacks below during stdlib init.
// Runtime class-specific helpers use those callbacks to bridge data between
// the two stream models; the historical generic functions remain as fallbacks
// for call sites where HIR did not preserve the stream class name.
// ─────────────────────────────────────────────────────────────────

type WebReadableNewFn = unsafe extern "C" fn(f64, f64, f64, f64) -> f64;
type WebReadableEnqueueFn = unsafe extern "C" fn(f64, f64) -> f64;
type WebReadableCloseFn = unsafe extern "C" fn(f64) -> f64;
type WebReadableErrorFn = unsafe extern "C" fn(f64, f64) -> f64;
type WebWritableNewFn = unsafe extern "C" fn(f64, f64, f64, f64, f64) -> f64;
type WebReadableGetReaderFn = unsafe extern "C" fn(f64) -> f64;
type WebReaderReadFn = unsafe extern "C" fn(f64) -> *mut crate::promise::Promise;
type WebWritableGetWriterFn = unsafe extern "C" fn(f64) -> f64;
type WebWriterWriteFn = unsafe extern "C" fn(f64, f64) -> *mut crate::promise::Promise;
type WebWriterCloseFn = unsafe extern "C" fn(f64) -> *mut crate::promise::Promise;
type WebWriterAbortFn = unsafe extern "C" fn(f64, f64) -> *mut crate::promise::Promise;

static WEB_READABLE_NEW_PTR: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static WEB_READABLE_ENQUEUE_PTR: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static WEB_READABLE_CLOSE_PTR: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static WEB_READABLE_ERROR_PTR: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static WEB_WRITABLE_NEW_PTR: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static WEB_READABLE_GET_READER_PTR: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static WEB_READER_READ_PTR: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static WEB_WRITABLE_GET_WRITER_PTR: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static WEB_WRITER_WRITE_PTR: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static WEB_WRITER_CLOSE_PTR: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static WEB_WRITER_ABORT_PTR: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

#[no_mangle]
pub unsafe extern "C" fn js_register_node_stream_web_adapter_callbacks(
    readable_new: WebReadableNewFn,
    readable_enqueue: WebReadableEnqueueFn,
    readable_close: WebReadableCloseFn,
    readable_error: WebReadableErrorFn,
    writable_new: WebWritableNewFn,
    readable_get_reader: WebReadableGetReaderFn,
    reader_read: WebReaderReadFn,
    writable_get_writer: WebWritableGetWriterFn,
    writer_write: WebWriterWriteFn,
    writer_close: WebWriterCloseFn,
    writer_abort: WebWriterAbortFn,
) {
    WEB_READABLE_NEW_PTR.store(readable_new as *mut (), Ordering::Release);
    WEB_READABLE_ENQUEUE_PTR.store(readable_enqueue as *mut (), Ordering::Release);
    WEB_READABLE_CLOSE_PTR.store(readable_close as *mut (), Ordering::Release);
    WEB_READABLE_ERROR_PTR.store(readable_error as *mut (), Ordering::Release);
    WEB_WRITABLE_NEW_PTR.store(writable_new as *mut (), Ordering::Release);
    WEB_READABLE_GET_READER_PTR.store(readable_get_reader as *mut (), Ordering::Release);
    WEB_READER_READ_PTR.store(reader_read as *mut (), Ordering::Release);
    WEB_WRITABLE_GET_WRITER_PTR.store(writable_get_writer as *mut (), Ordering::Release);
    WEB_WRITER_WRITE_PTR.store(writer_write as *mut (), Ordering::Release);
    WEB_WRITER_CLOSE_PTR.store(writer_close as *mut (), Ordering::Release);
    WEB_WRITER_ABORT_PTR.store(writer_abort as *mut (), Ordering::Release);
}

macro_rules! load_web_callback {
    ($slot:expr, $ty:ty) => {{
        let p = $slot.load(Ordering::Acquire);
        if p.is_null() {
            None
        } else {
            Some(unsafe { std::mem::transmute::<*mut (), $ty>(p) })
        }
    }};
}

fn web_readable_new() -> Option<WebReadableNewFn> {
    load_web_callback!(WEB_READABLE_NEW_PTR, WebReadableNewFn)
}

fn web_readable_enqueue() -> Option<WebReadableEnqueueFn> {
    load_web_callback!(WEB_READABLE_ENQUEUE_PTR, WebReadableEnqueueFn)
}

fn web_readable_close() -> Option<WebReadableCloseFn> {
    load_web_callback!(WEB_READABLE_CLOSE_PTR, WebReadableCloseFn)
}

fn web_readable_error() -> Option<WebReadableErrorFn> {
    load_web_callback!(WEB_READABLE_ERROR_PTR, WebReadableErrorFn)
}

fn web_writable_new() -> Option<WebWritableNewFn> {
    load_web_callback!(WEB_WRITABLE_NEW_PTR, WebWritableNewFn)
}

fn web_readable_get_reader() -> Option<WebReadableGetReaderFn> {
    load_web_callback!(WEB_READABLE_GET_READER_PTR, WebReadableGetReaderFn)
}

fn web_reader_read() -> Option<WebReaderReadFn> {
    load_web_callback!(WEB_READER_READ_PTR, WebReaderReadFn)
}

fn web_writable_get_writer() -> Option<WebWritableGetWriterFn> {
    load_web_callback!(WEB_WRITABLE_GET_WRITER_PTR, WebWritableGetWriterFn)
}

fn web_writer_write() -> Option<WebWriterWriteFn> {
    load_web_callback!(WEB_WRITER_WRITE_PTR, WebWriterWriteFn)
}

fn web_writer_close() -> Option<WebWriterCloseFn> {
    load_web_callback!(WEB_WRITER_CLOSE_PTR, WebWriterCloseFn)
}

fn web_writer_abort() -> Option<WebWriterAbortFn> {
    load_web_callback!(WEB_WRITER_ABORT_PTR, WebWriterAbortFn)
}

fn closure_value(closure: *mut ClosureHeader) -> f64 {
    f64::from_bits(JSValue::pointer(closure as *const u8).bits())
}

fn closure_with_stream(func: *const u8, node_stream: f64) -> f64 {
    let closure = js_closure_alloc(func, 1);
    js_closure_set_capture_f64(closure, 0, node_stream);
    closure_value(closure)
}

fn build_enumerable_object(fields: &[(&[u8], f64)]) -> f64 {
    let obj = js_object_alloc(0, fields.len() as u32);
    let mut keys = crate::array::js_array_alloc(fields.len() as u32);
    for (idx, (name, value)) in fields.iter().enumerate() {
        keys = crate::array::js_array_push_f64(keys, string_value(name));
        js_object_set_field(obj, idx as u32, JSValue::from_bits(value.to_bits()));
    }
    crate::object::js_object_set_keys(obj, keys);
    box_pointer(obj as *const u8)
}

fn build_web_read_result(value: f64, done: bool) -> f64 {
    build_enumerable_object(&[(b"value", value), (b"done", bool_value(done))])
}

fn property_value(value: f64, name: &[u8]) -> f64 {
    unsafe { crate::value::js_get_property(value, name.as_ptr() as i64, name.len() as i64) }
}

fn call_stream_callback(callback: f64, err: f64) {
    if !is_callable_value(callback) {
        return;
    }
    let arg = if err.to_bits() == TAG_UNDEFINED {
        f64::from_bits(TAG_NULL)
    } else {
        err
    };
    unsafe {
        let _ = crate::closure::js_native_call_value(callback, [arg].as_ptr(), 1);
    }
}

extern "C" fn node_to_web_readable_pull(closure: *const ClosureHeader, controller: f64) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let node_stream = js_closure_get_capture_f64(closure, 0);
    let chunk = read_stream_with_size_arg(node_stream, f64::from_bits(TAG_UNDEFINED));
    match chunk.to_bits() {
        TAG_NULL | TAG_UNDEFINED => {
            if stream_hidden_ended(node_stream) || !readable_chunks_nonempty(node_stream) {
                if let Some(close) = web_readable_close() {
                    unsafe {
                        close(controller);
                    }
                }
            }
        }
        _ => {
            if let Some(enqueue) = web_readable_enqueue() {
                unsafe {
                    enqueue(controller, chunk);
                }
            }
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn node_to_web_readable_cancel(closure: *const ClosureHeader, reason: f64) -> f64 {
    if !closure.is_null() {
        destroy_stream(js_closure_get_capture_f64(closure, 0), reason);
    }
    f64::from_bits(TAG_UNDEFINED)
}

fn node_readable_to_web(node_stream: f64) -> Option<f64> {
    let readable_new = web_readable_new()?;
    crate::closure::js_register_closure_arity(node_to_web_readable_pull as *const u8, 1);
    crate::closure::js_register_closure_arity(node_to_web_readable_cancel as *const u8, 1);
    let pull = js_closure_alloc(node_to_web_readable_pull as *const u8, 1);
    js_closure_set_capture_f64(pull, 0, node_stream);
    let cancel = js_closure_alloc(node_to_web_readable_cancel as *const u8, 1);
    js_closure_set_capture_f64(cancel, 0, node_stream);
    Some(unsafe {
        readable_new(
            f64::from_bits(TAG_UNDEFINED),
            closure_value(pull),
            closure_value(cancel),
            1.0,
        )
    })
}

extern "C" fn fallback_web_reader_read(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return resolved_promise(build_web_read_result(f64::from_bits(TAG_UNDEFINED), true));
    }
    let node_stream = js_closure_get_capture_f64(closure, 0);
    let chunk = read_stream_with_size_arg(node_stream, f64::from_bits(TAG_UNDEFINED));
    let result = match chunk.to_bits() {
        TAG_NULL | TAG_UNDEFINED => {
            let done = stream_hidden_ended(node_stream) || !readable_chunks_nonempty(node_stream);
            build_web_read_result(f64::from_bits(TAG_UNDEFINED), done)
        }
        _ => build_web_read_result(chunk, false),
    };
    resolved_promise(result)
}

extern "C" fn fallback_web_reader_cancel(closure: *const ClosureHeader, reason: f64) -> f64 {
    if !closure.is_null() {
        destroy_stream(js_closure_get_capture_f64(closure, 0), reason);
    }
    resolved_promise(f64::from_bits(TAG_UNDEFINED))
}

extern "C" fn fallback_web_readable_get_reader(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let node_stream = js_closure_get_capture_f64(closure, 0);
    crate::closure::js_register_closure_arity(fallback_web_reader_read as *const u8, 0);
    crate::closure::js_register_closure_arity(fallback_web_reader_cancel as *const u8, 1);
    build_enumerable_object(&[
        (
            b"read",
            closure_with_stream(fallback_web_reader_read as *const u8, node_stream),
        ),
        (
            b"cancel",
            closure_with_stream(fallback_web_reader_cancel as *const u8, node_stream),
        ),
    ])
}

fn fallback_node_readable_to_web(node_stream: f64) -> f64 {
    crate::closure::js_register_closure_arity(fallback_web_readable_get_reader as *const u8, 0);
    crate::closure::js_register_closure_arity(fallback_web_reader_cancel as *const u8, 1);
    build_enumerable_object(&[
        (
            b"getReader",
            closure_with_stream(fallback_web_readable_get_reader as *const u8, node_stream),
        ),
        (
            b"cancel",
            closure_with_stream(fallback_web_reader_cancel as *const u8, node_stream),
        ),
    ])
}

extern "C" fn node_to_web_writable_write(closure: *const ClosureHeader, chunk: f64) -> f64 {
    if !closure.is_null() {
        let node_stream = js_closure_get_capture_f64(closure, 0);
        let _ = write_writable_chunk(
            node_stream,
            chunk,
            f64::from_bits(TAG_UNDEFINED),
            f64::from_bits(TAG_UNDEFINED),
        );
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn node_to_web_writable_close(closure: *const ClosureHeader) -> f64 {
    if !closure.is_null() {
        let node_stream = js_closure_get_capture_f64(closure, 0);
        finish_stream_with_args(
            node_stream,
            f64::from_bits(TAG_UNDEFINED),
            f64::from_bits(TAG_UNDEFINED),
            f64::from_bits(TAG_UNDEFINED),
        );
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn node_to_web_writable_abort(closure: *const ClosureHeader, reason: f64) -> f64 {
    if !closure.is_null() {
        destroy_stream(js_closure_get_capture_f64(closure, 0), reason);
    }
    f64::from_bits(TAG_UNDEFINED)
}

fn node_writable_to_web(node_stream: f64) -> Option<f64> {
    let writable_new = web_writable_new()?;
    crate::closure::js_register_closure_arity(node_to_web_writable_write as *const u8, 1);
    crate::closure::js_register_closure_arity(node_to_web_writable_close as *const u8, 0);
    crate::closure::js_register_closure_arity(node_to_web_writable_abort as *const u8, 1);
    let write = js_closure_alloc(node_to_web_writable_write as *const u8, 1);
    js_closure_set_capture_f64(write, 0, node_stream);
    let close = js_closure_alloc(node_to_web_writable_close as *const u8, 1);
    js_closure_set_capture_f64(close, 0, node_stream);
    let abort = js_closure_alloc(node_to_web_writable_abort as *const u8, 1);
    js_closure_set_capture_f64(abort, 0, node_stream);
    Some(unsafe {
        writable_new(
            f64::from_bits(TAG_UNDEFINED),
            closure_value(write),
            closure_value(close),
            closure_value(abort),
            1.0,
        )
    })
}

extern "C" fn fallback_web_writer_write(closure: *const ClosureHeader, chunk: f64) -> f64 {
    if !closure.is_null() {
        let node_stream = js_closure_get_capture_f64(closure, 0);
        let _ = write_writable_chunk(
            node_stream,
            chunk,
            f64::from_bits(TAG_UNDEFINED),
            f64::from_bits(TAG_UNDEFINED),
        );
    }
    resolved_promise(f64::from_bits(TAG_UNDEFINED))
}

extern "C" fn fallback_web_writer_close(closure: *const ClosureHeader) -> f64 {
    if !closure.is_null() {
        finish_stream_with_args(
            js_closure_get_capture_f64(closure, 0),
            f64::from_bits(TAG_UNDEFINED),
            f64::from_bits(TAG_UNDEFINED),
            f64::from_bits(TAG_UNDEFINED),
        );
    }
    resolved_promise(f64::from_bits(TAG_UNDEFINED))
}

extern "C" fn fallback_web_writer_abort(closure: *const ClosureHeader, reason: f64) -> f64 {
    if !closure.is_null() {
        destroy_stream(js_closure_get_capture_f64(closure, 0), reason);
    }
    resolved_promise(f64::from_bits(TAG_UNDEFINED))
}

extern "C" fn fallback_web_writable_get_writer(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let node_stream = js_closure_get_capture_f64(closure, 0);
    crate::closure::js_register_closure_arity(fallback_web_writer_write as *const u8, 1);
    crate::closure::js_register_closure_arity(fallback_web_writer_close as *const u8, 0);
    crate::closure::js_register_closure_arity(fallback_web_writer_abort as *const u8, 1);
    build_enumerable_object(&[
        (
            b"write",
            closure_with_stream(fallback_web_writer_write as *const u8, node_stream),
        ),
        (
            b"close",
            closure_with_stream(fallback_web_writer_close as *const u8, node_stream),
        ),
        (
            b"abort",
            closure_with_stream(fallback_web_writer_abort as *const u8, node_stream),
        ),
    ])
}

fn fallback_node_writable_to_web(node_stream: f64) -> f64 {
    crate::closure::js_register_closure_arity(fallback_web_writable_get_writer as *const u8, 0);
    crate::closure::js_register_closure_arity(fallback_web_writer_abort as *const u8, 1);
    build_enumerable_object(&[
        (
            b"getWriter",
            closure_with_stream(fallback_web_writable_get_writer as *const u8, node_stream),
        ),
        (
            b"abort",
            closure_with_stream(fallback_web_writer_abort as *const u8, node_stream),
        ),
    ])
}

pub(crate) fn js_node_stream_readable_to_web_method_value(node_stream: f64) -> f64 {
    fallback_node_readable_to_web(node_stream)
}

pub(crate) fn js_node_stream_writable_to_web_method_value(node_stream: f64) -> f64 {
    fallback_node_writable_to_web(node_stream)
}

pub(crate) fn js_node_stream_duplex_to_web_method_value(node_stream: f64) -> f64 {
    web_pair_object(
        fallback_node_readable_to_web(node_stream),
        fallback_node_writable_to_web(node_stream),
    )
}

fn web_pair_object(readable: f64, writable: f64) -> f64 {
    build_enumerable_object(&[(b"readable", readable), (b"writable", writable)])
}

fn install_web_readable_adapter(node_stream: f64, web_stream: f64) -> bool {
    let Some(get_reader) = web_readable_get_reader() else {
        return false;
    };
    let reader = unsafe { get_reader(web_stream) };
    if reader.to_bits() == TAG_UNDEFINED {
        return false;
    }
    crate::closure::js_register_closure_arity(web_to_node_readable_read as *const u8, 1);
    let read = js_closure_alloc(web_to_node_readable_read as *const u8, 2);
    js_closure_set_capture_f64(read, 0, node_stream);
    js_closure_set_capture_f64(read, 1, reader);
    set_hidden_value(node_stream, hidden_read_key(), closure_value(read));
    set_hidden_value(
        node_stream,
        hidden_default_read_error_key(),
        f64::from_bits(TAG_FALSE),
    );
    true
}

extern "C" fn web_to_node_readable_read(closure: *const ClosureHeader, _size: f64) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let node_stream = js_closure_get_capture_f64(closure, 0);
    let reader = js_closure_get_capture_f64(closure, 1);
    if has_truthy_hidden(node_stream, hidden_key(b"webReadablePumping")) {
        return f64::from_bits(TAG_UNDEFINED);
    }
    set_hidden_value(
        node_stream,
        hidden_key(b"webReadablePumping"),
        f64::from_bits(TAG_TRUE),
    );
    pump_web_reader(node_stream, reader);
    f64::from_bits(TAG_UNDEFINED)
}

fn pump_web_reader(node_stream: f64, reader: f64) {
    if stream_destroyed(node_stream) || stream_hidden_ended(node_stream) {
        return;
    }
    let Some(read) = web_reader_read() else {
        return;
    };
    let promise = unsafe { read(reader) };
    if promise.is_null() {
        return;
    }
    crate::closure::js_register_closure_arity(web_to_node_readable_read_fulfilled as *const u8, 1);
    crate::closure::js_register_closure_arity(web_to_node_readable_read_rejected as *const u8, 1);
    let fulfilled = js_closure_alloc(web_to_node_readable_read_fulfilled as *const u8, 2);
    js_closure_set_capture_f64(fulfilled, 0, node_stream);
    js_closure_set_capture_f64(fulfilled, 1, reader);
    let rejected = js_closure_alloc(web_to_node_readable_read_rejected as *const u8, 1);
    js_closure_set_capture_f64(rejected, 0, node_stream);
    crate::promise::js_promise_attach_handlers(promise, fulfilled, rejected);
}

extern "C" fn web_to_node_readable_read_fulfilled(
    closure: *const ClosureHeader,
    result: f64,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let node_stream = js_closure_get_capture_f64(closure, 0);
    let reader = js_closure_get_capture_f64(closure, 1);
    let done = property_value(result, b"done");
    if crate::value::js_is_truthy(done) != 0 {
        set_hidden_value(
            node_stream,
            hidden_key(b"webReadablePumping"),
            f64::from_bits(TAG_FALSE),
        );
        let _ = push_chunk(node_stream, f64::from_bits(TAG_NULL));
        return f64::from_bits(TAG_UNDEFINED);
    }
    let value = property_value(result, b"value");
    let _ = push_chunk(node_stream, value);
    pump_web_reader(node_stream, reader);
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn web_to_node_readable_read_rejected(
    closure: *const ClosureHeader,
    reason: f64,
) -> f64 {
    if !closure.is_null() {
        let node_stream = js_closure_get_capture_f64(closure, 0);
        set_hidden_value(
            node_stream,
            hidden_key(b"webReadablePumping"),
            f64::from_bits(TAG_FALSE),
        );
        destroy_stream(node_stream, reason);
    }
    f64::from_bits(TAG_UNDEFINED)
}

fn install_web_writable_adapter(node_stream: f64, web_stream: f64) -> bool {
    let Some(get_writer) = web_writable_get_writer() else {
        return false;
    };
    let writer = unsafe { get_writer(web_stream) };
    if writer.to_bits() == TAG_UNDEFINED {
        return false;
    }
    crate::closure::js_register_closure_arity(web_to_node_writable_write as *const u8, 3);
    crate::closure::js_register_closure_arity(web_to_node_writable_final as *const u8, 1);
    crate::closure::js_register_closure_arity(web_to_node_writable_destroy as *const u8, 2);
    let write = js_closure_alloc(web_to_node_writable_write as *const u8, 1);
    js_closure_set_capture_f64(write, 0, writer);
    let final_cb = js_closure_alloc(web_to_node_writable_final as *const u8, 1);
    js_closure_set_capture_f64(final_cb, 0, writer);
    let destroy = js_closure_alloc(web_to_node_writable_destroy as *const u8, 1);
    js_closure_set_capture_f64(destroy, 0, writer);
    set_hidden_value(node_stream, hidden_write_key(), closure_value(write));
    set_hidden_value(
        node_stream,
        hidden_writable_final_key(),
        closure_value(final_cb),
    );
    set_hidden_value(
        node_stream,
        hidden_writable_final_invoked_key(),
        f64::from_bits(TAG_FALSE),
    );
    set_hidden_value(
        node_stream,
        hidden_writable_final_pending_key(),
        f64::from_bits(TAG_FALSE),
    );
    set_hidden_value(
        node_stream,
        hidden_key(STREAM_DESTROY_KEY),
        closure_value(destroy),
    );
    true
}

extern "C" fn web_to_node_writable_write(
    closure: *const ClosureHeader,
    chunk: f64,
    _encoding: f64,
    callback: f64,
) -> f64 {
    if closure.is_null() {
        call_stream_callback(callback, f64::from_bits(TAG_UNDEFINED));
        return f64::from_bits(TAG_UNDEFINED);
    }
    let writer = js_closure_get_capture_f64(closure, 0);
    if let Some(write) = web_writer_write() {
        let promise = unsafe { write(writer, chunk) };
        attach_web_writable_callback(promise, callback);
    } else {
        call_stream_callback(callback, f64::from_bits(TAG_UNDEFINED));
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn web_to_node_writable_final(closure: *const ClosureHeader, callback: f64) -> f64 {
    if closure.is_null() {
        call_stream_callback(callback, f64::from_bits(TAG_UNDEFINED));
        return f64::from_bits(TAG_UNDEFINED);
    }
    let writer = js_closure_get_capture_f64(closure, 0);
    if let Some(close) = web_writer_close() {
        let promise = unsafe { close(writer) };
        attach_web_writable_callback(promise, callback);
    } else {
        call_stream_callback(callback, f64::from_bits(TAG_UNDEFINED));
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn web_to_node_writable_destroy(
    closure: *const ClosureHeader,
    err: f64,
    callback: f64,
) -> f64 {
    if closure.is_null() {
        call_stream_callback(callback, f64::from_bits(TAG_UNDEFINED));
        return f64::from_bits(TAG_UNDEFINED);
    }
    let writer = js_closure_get_capture_f64(closure, 0);
    if let Some(abort) = web_writer_abort() {
        let reason = if err.to_bits() == TAG_NULL {
            f64::from_bits(TAG_UNDEFINED)
        } else {
            err
        };
        let promise = unsafe { abort(writer, reason) };
        attach_web_writable_callback(promise, callback);
    } else {
        call_stream_callback(callback, f64::from_bits(TAG_UNDEFINED));
    }
    f64::from_bits(TAG_UNDEFINED)
}

fn attach_web_writable_callback(promise: *mut crate::promise::Promise, callback: f64) {
    if promise.is_null() {
        call_stream_callback(callback, f64::from_bits(TAG_UNDEFINED));
        return;
    }
    crate::closure::js_register_closure_arity(web_to_node_writable_fulfilled as *const u8, 1);
    crate::closure::js_register_closure_arity(web_to_node_writable_rejected as *const u8, 1);
    let fulfilled = js_closure_alloc(web_to_node_writable_fulfilled as *const u8, 1);
    js_closure_set_capture_f64(fulfilled, 0, callback);
    let rejected = js_closure_alloc(web_to_node_writable_rejected as *const u8, 1);
    js_closure_set_capture_f64(rejected, 0, callback);
    crate::promise::js_promise_attach_handlers(promise, fulfilled, rejected);
}

extern "C" fn web_to_node_writable_fulfilled(closure: *const ClosureHeader, _value: f64) -> f64 {
    if !closure.is_null() {
        call_stream_callback(
            js_closure_get_capture_f64(closure, 0),
            f64::from_bits(TAG_UNDEFINED),
        );
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn web_to_node_writable_rejected(closure: *const ClosureHeader, reason: f64) -> f64 {
    if !closure.is_null() {
        call_stream_callback(js_closure_get_capture_f64(closure, 0), reason);
    }
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_node_stream_readable_to_web(node_stream: f64) -> f64 {
    node_readable_to_web(node_stream).unwrap_or_else(|| fallback_node_readable_to_web(node_stream))
}

#[no_mangle]
pub extern "C" fn js_node_stream_writable_to_web(node_stream: f64) -> f64 {
    node_writable_to_web(node_stream).unwrap_or_else(|| fallback_node_writable_to_web(node_stream))
}

#[no_mangle]
pub extern "C" fn js_node_stream_duplex_to_web(node_stream: f64) -> f64 {
    match (
        node_readable_to_web(node_stream),
        node_writable_to_web(node_stream),
    ) {
        (Some(readable), Some(writable)) => web_pair_object(readable, writable),
        _ => web_pair_object(
            fallback_node_readable_to_web(node_stream),
            fallback_node_writable_to_web(node_stream),
        ),
    }
}

#[no_mangle]
pub extern "C" fn js_node_stream_readable_from_web(web_stream: f64, opts: f64) -> f64 {
    let readable = js_node_stream_readable_new(readable_from_options(opts));
    if install_web_readable_adapter(readable, web_stream) {
        readable
    } else {
        js_node_stream_from_web(web_stream)
    }
}

#[no_mangle]
pub extern "C" fn js_node_stream_writable_from_web(web_stream: f64, opts: f64) -> f64 {
    let writable = js_node_stream_writable_new(opts);
    if install_web_writable_adapter(writable, web_stream) {
        writable
    } else {
        js_node_stream_from_web(web_stream)
    }
}

#[no_mangle]
pub extern "C" fn js_node_stream_duplex_from_web(pair: f64, opts: f64) -> f64 {
    let readable_web = property_value(pair, b"readable");
    let writable_web = property_value(pair, b"writable");
    let duplex = js_node_stream_duplex_new(opts);
    let readable_ok = readable_web.to_bits() != TAG_UNDEFINED
        && install_web_readable_adapter(duplex, readable_web);
    let writable_ok = writable_web.to_bits() != TAG_UNDEFINED
        && install_web_writable_adapter(duplex, writable_web);
    if writable_ok {
        set_hidden_value(
            duplex,
            hidden_key(b"writableCustomSink"),
            f64::from_bits(TAG_TRUE),
        );
    }
    if readable_ok || writable_ok {
        duplex
    } else {
        js_node_stream_from_web(pair)
    }
}

/// A WHATWG-stream-shaped stub: an object carrying both `getReader` and
/// `getWriter` method stubs. A real `ReadableStream` only has `getReader`
/// and a `WritableStream` only `getWriter`, but the single `js_node_stream_to_web`
/// entry can't tell which class `.toWeb` was called on (the NativeMethodCall
/// drops the class), so the union shape lets `Readable.toWeb`,
/// `Writable.toWeb`, and the `{ readable, writable }` pair from
/// `Duplex.toWeb` all satisfy their `typeof x.getReader/getWriter === "function"`
/// existence checks. Data isn't forwarded between the Node and WHATWG
/// universes — that's the remaining #1540 gap.
pub(super) fn build_web_stream_stub() -> f64 {
    let methods: [(&str, StubFn); 2] = [
        ("getReader", cast0(ns_undefined0)),
        ("getWriter", cast0(ns_undefined0)),
    ];
    let obj = build_object(&methods, WEB_STREAM_SHAPE_ID + methods.len() as u32);
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

/// `Readable.toWeb` / `Writable.toWeb` / `Duplex.toWeb` — returns a
/// web-stream-shaped stub (#1540). For Duplex the result also exposes
/// `readable` / `writable` web-stream stubs so `pair.readable.getReader`
/// / `pair.writable.getWriter` resolve.
#[no_mangle]
pub extern "C" fn js_node_stream_to_web(node_stream: f64) -> f64 {
    let readable = get_hidden_value(node_stream, hidden_readable_flag_key()).is_some();
    let writable = get_hidden_value(node_stream, hidden_writable_flag_key()).is_some();
    match (readable, writable) {
        (true, true) => return js_node_stream_duplex_to_web(node_stream),
        (true, false) => return js_node_stream_readable_to_web(node_stream),
        (false, true) => return js_node_stream_writable_to_web(node_stream),
        (false, false) => {}
    }

    let top = build_web_stream_stub();
    set_hidden_value(top, hidden_key(b"readable"), build_web_stream_stub());
    set_hidden_value(top, hidden_key(b"writable"), build_web_stream_stub());
    top
}

/// Generic `.fromWeb` fallback used when the lowering cannot preserve the
/// static stream class. Prefer real adapters when the input shape makes a
/// direction clear, then fall back to the legacy Duplex stub.
#[no_mangle]
pub extern "C" fn js_node_stream_from_web(web_stream: f64) -> f64 {
    let readable_web = property_value(web_stream, b"readable");
    let writable_web = property_value(web_stream, b"writable");
    if readable_web.to_bits() != TAG_UNDEFINED || writable_web.to_bits() != TAG_UNDEFINED {
        return js_node_stream_duplex_from_web(web_stream, f64::from_bits(TAG_UNDEFINED));
    }

    let readable = js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED));
    if install_web_readable_adapter(readable, web_stream) {
        return readable;
    }

    let writable = js_node_stream_writable_new(f64::from_bits(TAG_UNDEFINED));
    if install_web_writable_adapter(writable, web_stream) {
        return writable;
    }

    js_node_stream_duplex_new(f64::from_bits(TAG_UNDEFINED))
}
