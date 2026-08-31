//! node:stream — the `js_node_stream_*_new` / `*_subclass_init` constructors
//! and `Readable.from` factory (split out of node_stream_constructors.rs for
//! the 2000-line file-size gate, #1987).
use super::*;
use crate::closure::ClosureHeader;
use crate::object::{js_object_get_field_by_name_f64, js_object_set_field_by_name, ObjectHeader};
use crate::value::JSValue;

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

/// Initialize a source-compiled subclass of EventEmitterAsyncResource on its
/// already-allocated `this` object. The listener surface remains the generic
/// object-backed EventEmitter implementation; a hidden AsyncResource supplies
/// the execution scope, lifecycle, ids, and `asyncResource.eventEmitter`
/// back-reference.
#[no_mangle]
pub extern "C" fn js_event_emitter_async_resource_subclass_init(this: f64, options: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let this_handle = scope.root_nanbox_f64(this);
    let options_handle = scope.root_nanbox_f64(options);
    js_event_emitter_subclass_init(this_handle.get_nanbox_f64());

    let this = this_handle.get_nanbox_f64();
    let raw = raw_ptr_from_value(this);
    if raw == 0 || unsafe { gc_type_for_ptr(raw) } != Some(crate::gc::GC_TYPE_OBJECT) {
        return this;
    }
    let options = options_handle.get_nanbox_f64();
    let options_value = JSValue::from_bits(options.to_bits());
    let mut name = if options_value.is_any_string() {
        options
    } else {
        let key = crate::string::js_string_from_bytes(b"name".as_ptr(), 4);
        let options = options_handle.get_nanbox_f64();
        let options_obj = raw_ptr_from_value(options) as *const ObjectHeader;
        if options_obj.is_null() {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        } else {
            crate::object::js_object_get_field_by_name_f64(options_obj, key)
        }
    };
    if JSValue::from_bits(name.to_bits()).is_undefined() {
        let current_obj = raw_ptr_from_value(this_handle.get_nanbox_f64()) as *mut ObjectHeader;
        let class_id = unsafe { (*current_obj).class_id };
        let default_name = crate::object::class_name_for_id(class_id)
            .unwrap_or_else(|| "EventEmitterAsyncResource".to_string());
        let name_ptr =
            crate::string::js_string_from_bytes(default_name.as_ptr(), default_name.len() as u32);
        name = f64::from_bits(JSValue::string_ptr(name_ptr).bits());
    }
    let name_handle = scope.root_nanbox_f64(name);
    let async_options = if options_value.is_any_string() {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    } else {
        options_handle.get_nanbox_f64()
    };
    let resource =
        crate::async_hooks::js_async_resource_new(name_handle.get_nanbox_f64(), async_options);
    let obj = this_handle.get_nanbox_f64();
    let raw = raw_ptr_from_value(obj);
    crate::async_hooks::js_async_resource_set_event_emitter(resource, raw as i64);
    unsafe {
        crate::object::js_object_set_field_by_name(
            raw as *mut ObjectHeader,
            hidden_key(EVENT_EMITTER_ASYNC_RESOURCE_KEY),
            f64::from_bits(crate::value::js_nanbox_pointer(resource).to_bits()),
        );
        install_event_emitter_async_resource_instance_methods(
            raw as *mut ObjectHeader,
            this_handle.get_nanbox_f64(),
        );
    }
    this_handle.get_nanbox_f64()
}

/// `super(n)` for a source-compiled `class X extends Array` (e.g. lru-cache's
/// `ZeroArray`: `class ZeroArray extends Array { constructor(n){ super(n);
/// this.fill(0) } }`). Perry models the subclass instance as a plain object,
/// not a real exotic Array, so `super(n)` initializes its elements store. In
/// the default representation, inherited methods resolve through
/// `Array.prototype` and are not stamped as enumerable own properties. The
/// legacy shape-carried kill switch retains its old compatibility closure.
/// The codegen `super()` lowering calls this entry point.
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
    if crate::array::subclass_elements::array_subclass_elements_enabled() {
        // Elements-backed instance: `length` and the indices live in the
        // store, never as shape-carried properties.
        let scope = crate::gc::RuntimeHandleScope::new();
        let this_root = scope.root_nanbox_f64(this);
        unsafe {
            crate::array::subclass_elements::install_elements(obj, len.min(u32::MAX as f64) as u32)
        };
        return this_root.get_nanbox_f64();
    }
    let length_key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
    js_object_set_field_by_name(obj, length_key, len);
    crate::closure::js_register_closure_arity(ns_array_fill as *const u8, 3);
    let methods: [(&str, StubFn); 1] = [("fill", super::cast3(ns_array_fill))];
    install_methods_on_existing_object(obj, this, &methods, &[]);
    this
}

/// Array's overloaded constructor semantics for a source-compiled subclass.
/// One numeric argument is a length; every other argument list becomes the
/// initial indexed elements.
#[no_mangle]
pub unsafe extern "C" fn js_array_subclass_init_args(
    this: f64,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let args = if args_ptr.is_null() || args_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(args_ptr, args_len)
    };
    if args.len() == 1 && JSValue::from_bits(args[0].to_bits()).is_number() {
        return js_array_subclass_init(this, args[0]);
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(this);
    let args = scope.root_nanbox_f64_slice(args);
    js_array_subclass_init(this.get_nanbox_f64(), args.len() as f64);
    for (index, value) in args.iter().enumerate() {
        let name = index.to_string();
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let receiver = this.get_nanbox_f64();
        let raw = raw_ptr_from_value(receiver) as *mut ObjectHeader;
        if !raw.is_null() {
            js_object_set_field_by_name(raw, key, value.get_nanbox_f64());
        }
    }
    this.get_nanbox_f64()
}

/// Legacy shape-carried compatibility closure for `Array.prototype.fill`.
pub(super) extern "C" fn ns_array_fill(
    closure: *const ClosureHeader,
    value: f64,
    start: f64,
    end: f64,
) -> f64 {
    // `fill(value, start?, end?)`. An omitted argument arrives as `undefined`
    // and selects the spec default (`0` / `length`); before this the stub had
    // arity 1, so `sub.fill(8, 1)` filled the WHOLE array instead of the tail
    // from index 1 (node: `7|8|8`, perry: `8|8|8`).
    let present = |v: f64| i32::from(!JSValue::from_bits(v.to_bits()).is_undefined());
    crate::array::js_array_fill_generic(
        super::this_value(closure),
        value,
        present(start),
        start,
        present(end),
        end,
    )
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
    if is_invalid_readable_from_input(iterable) {
        throw_readable_from_invalid_iterable(iterable);
    }
    let readable = js_node_stream_readable_new(readable_from_options(opts));
    let raw = raw_ptr_from_value(readable);
    if raw >= 0x10000 {
        // Armed in a C trampoline frame (#9305); both continuations run
        // after the trap is popped, as before.
        match crate::exception::catch_js_throw(|| normalize_readable_from_input(iterable)) {
            Ok(normalized) => {
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
            }
            Err(err) => {
                destroy_stream(readable, err);
            }
        }
    }
    readable
}
