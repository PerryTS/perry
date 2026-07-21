//! node:stream — the `js_node_stream_*_new` / `*_subclass_init` constructors
//! and `Readable.from` factory (split out of node_stream_constructors.rs for
//! the 2000-line file-size gate, #1987).
use super::super::*;
use super::*;
use crate::closure::ClosureHeader;
use crate::object::{js_object_get_field_by_name_f64, js_object_set_field_by_name, ObjectHeader};
use crate::value::JSValue;
use std::os::raw::c_int;

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

/// `super(n)` for a source-compiled `class X extends Array` (e.g. lru-cache's
/// `ZeroArray`: `class ZeroArray extends Array { constructor(n){ super(n);
/// this.fill(0) } }`). Perry models the subclass instance as a plain object,
/// not a real exotic Array, so `super(n)` otherwise left it length-less with no
/// Array methods. Size it (`length = ToLength(n)`, a visible own property the
/// generic array-like helpers read) and install the Array surface the instance
/// relies on — currently `fill`, which delegates to `js_array_fill_generic`
/// (it operates on the receiver's own `length` + indexed properties, exactly
/// what an array-like object exposes). Indexed get/set already work as ordinary
/// object properties. Mirrors `js_event_emitter_subclass_init` (#5494); the
/// codegen `super()` lowering for an `Array` parent calls this. Additional
/// Array methods can be added to `array_subclass_methods` as bundles need them.
#[no_mangle]
pub extern "C" fn js_array_subclass_init(this: f64, n: f64) -> f64 {
    let raw = raw_ptr_from_value(this);
    if raw == 0 {
        return this;
    }
    if unsafe { gc_type_for_ptr(raw) } != Some(crate::gc::GC_TYPE_OBJECT) {
        return this;
    }
    let obj = raw as *mut ObjectHeader;
    // ToLength(n): undefined / NaN / <= 0 → 0; +Infinity (and any value past the
    // max array length) clamps to 2^53 - 1; otherwise floor(n).
    let len = {
        const MAX_SAFE_INTEGER: f64 = 9007199254740991.0; // 2^53 - 1
        let nv = JSValue::from_bits(n.to_bits());
        if nv.is_undefined() || n.is_nan() || n <= 0.0 {
            0.0
        } else if n.is_infinite() {
            MAX_SAFE_INTEGER
        } else {
            n.floor().min(MAX_SAFE_INTEGER)
        }
    };
    let length_key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
    js_object_set_field_by_name(obj, length_key, len);
    crate::closure::js_register_closure_arity(ns_array_fill as *const u8, 1);
    let methods: [(&str, StubFn); 1] = [("fill", super::cast1(ns_array_fill))];
    install_methods_on_existing_object(obj, this, &methods, &[]);
    this
}

/// `Array.prototype.fill`-equivalent installed on an Array-subclass instance:
/// fills the receiver's own indexed slots `0..length` with `value`. Delegates
/// to the generic array-like fill (which reads `length` off the receiver).
pub(super) extern "C" fn ns_array_fill(closure: *const ClosureHeader, value: f64) -> f64 {
    crate::array::js_array_fill_generic(super::this_value(closure), value, 0, 0.0, 0, 0.0)
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
