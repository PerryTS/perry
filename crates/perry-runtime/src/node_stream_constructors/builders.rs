//! node:stream — the `js_node_stream_*_new` / `*_subclass_init` constructors
//! and `Readable.from` factory (split out of node_stream_constructors.rs for
//! the 2000-line file-size gate, #1987).
use super::*;
use crate::closure::ClosureHeader;
use crate::object::ObjectHeader;
use crate::value::JSValue;

/// G1: an instance owns only its state; the methods are inherited from the
/// stream prototypes (`proto_methods.rs`). Who constructs it decides only
/// where a user hook may come from: a direct `new X(opts)` reads the options,
/// a subclass `super(opts)` (or `X.call(this, opts)`) also reads the class's
/// own `_read`/`_write`/... overrides on its prototype chain.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum StreamInit {
    Direct,
    Subclass,
}

/// A user-supplied stream hook: callable, and not a runtime builtin (a
/// payload family's prototype carries builtin `_transform`/`_flush` for
/// `super._transform(...)` parity; those are the hooks themselves, not an
/// override of them).
fn user_hook(value: f64) -> Option<f64> {
    if !is_callable_value(value) {
        return None;
    }
    let raw = raw_ptr_from_value(value);
    if raw >= 0x10000 && crate::closure::is_closure_ptr(raw) {
        let info = unsafe { (*(raw as *const ClosureHeader)).info };
        if !info.is_null() && unsafe { (*info).flags } & crate::closure::FN_BUILTIN != 0 {
            return None;
        }
    }
    Some(value)
}

/// The value of `name` on the receiver's chain, when it is a user hook.
fn subclass_hook(this: f64, name: crate::runtime_state_key::NamedStateKey) -> Option<f64> {
    user_hook(name.read_value(this))
}

/// node's `Readable` constructor body on an allocated object.
pub(crate) fn init_readable_in_place(
    this: f64,
    opts: f64,
    how: StreamInit,
    provided: Option<super::birth_memo::Input>,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(this);
    let opts = scope.root_nanbox_f64(opts);
    let t = &this;
    let o = &opts;
    let birth = provided.or_else(|| {
        if how == StreamInit::Subclass {
            super::birth_memo::begin_existing(
                t.get_nanbox_f64(),
                o.get_nanbox_f64(),
                super::birth::Kind::Readable,
            )
        } else {
            None
        }
    });
    if get_hidden_value(t.get_nanbox_f64(), crate::runtime_state_key!(b"_events")).is_none() {
        let events = scope.root_nanbox_f64(new_events_object());
        stream_birth_field!(
            t.get_nanbox_f64(),
            crate::runtime_state_key!(b"_events"),
            events.get_nanbox_f64()
        );
    }
    install_stream_state_layout(t.get_nanbox_f64());
    init_lifecycle_state(&this, &opts);
    init_readable_state(&this, &opts, false);
    if let Some(read) = read_callback_from_options(&opts) {
        let read = scope.root_nanbox_f64(read);
        stream_birth_field!(
            t.get_nanbox_f64(),
            crate::runtime_state_key!(b"_read"),
            read.get_nanbox_f64()
        );
        set_hidden_value(t.get_nanbox_f64(), hidden_read_key(), read.get_nanbox_f64());
    } else if how == StreamInit::Subclass {
        if let Some(read) = subclass_hook(t.get_nanbox_f64(), crate::runtime_state_key!(b"_read")) {
            let read = scope.root_nanbox_f64(read);
            set_hidden_value(t.get_nanbox_f64(), hidden_read_key(), read.get_nanbox_f64());
        }
    } else if how == StreamInit::Direct {
        set_hidden_value(
            t.get_nanbox_f64(),
            hidden_default_read_error_key(),
            f64::from_bits(TAG_TRUE),
        );
    }
    install_common_lifecycle_callbacks(&this, &opts);
    invoke_construct_callback(&this, &opts, birth);
    if provided.is_none() && how == StreamInit::Subclass {
        super::birth_memo::finish_existing(t.get_nanbox_f64(), birth, super::birth::Kind::Readable);
    }
}

#[no_mangle]
pub extern "C" fn js_node_stream_readable_new(opts: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let opts = scope.root_nanbox_f64(opts);
    let (readable, proof) = proto_methods::alloc_stream_instance(super::birth::Kind::Readable);
    let readable = scope.root_nanbox_f64(readable);
    init_readable_in_place(
        readable.get_nanbox_f64(),
        opts.get_nanbox_f64(),
        StreamInit::Direct,
        proof,
    );
    readable.get_nanbox_f64()
}

#[no_mangle]
pub extern "C" fn js_node_stream_readable_subclass_init(this: f64, opts: f64) -> f64 {
    if object_ptr_from_value(this).is_none() {
        return this;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(this);
    init_readable_in_place(this.get_nanbox_f64(), opts, StreamInit::Subclass, None);
    this.get_nanbox_f64()
}

/// #5137: `super()` for a source-compiled `class X extends EventEmitter`
/// (from `node:events`), and `EventEmitter.call(this)`. Gives `this` node's
/// instance state; `.on`/`.emit`/`.once`/… are inherited from the shared
/// `EventEmitter.prototype` (`install_event_emitter_prototype`), as in node.
/// This is the EventEmitter analog of
/// `js_node_stream_readable_subclass_init`; commander's `Command extends
/// EventEmitter` reaches it when its real npm source is compiled (the
/// package is in `perry.compilePackages`, so the `new Command()` → native
/// `js_commander_*` shim path is deliberately off). Unlike the stream
/// inits there is no option-driven state to seed — a plain EventEmitter
/// has no `_read`/`highWaterMark`/etc.
#[no_mangle]
pub extern "C" fn js_event_emitter_subclass_init(this: f64, options: f64) -> f64 {
    let raw = raw_ptr_from_value(this);
    if raw == 0 {
        return this;
    }
    if !matches!(unsafe { gc_type_for_ptr(raw) }, Some(crate::gc_object_type_pat!())) {
        return this;
    }
    // node's `EventEmitter.init`: own `_events`/`_eventsCount`/`_maxListeners`
    // only. The methods are inherited from the shared `EventEmitter.prototype`
    // (a subclass override on its own prototype shadows them there, and
    // `super.m()` finds the base through the chain), so nothing is installed
    // per instance (#10508).
    let scope = crate::gc::RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(this);
    let options = scope.root_nanbox_f64(options);
    let birth = super::birth_memo::begin_emitter(this.get_nanbox_f64(), options.get_nanbox_f64());
    init_event_emitter_state_with_presence(this.get_nanbox_f64(), birth);
    init_event_emitter_capture(this.get_nanbox_f64(), options.get_nanbox_f64());
    super::birth_memo::finish_emitter(this.get_nanbox_f64(), birth);
    this.get_nanbox_f64()
}

/// #10798: install the legacy `node:stream` `Stream` base surface onto
/// `this` for a source-compiled `class X extends Stream` (the bare
/// `node:stream` base — NOT one of its Readable/Writable/Duplex/Transform
/// subclasses). In Node, `Stream` is EventEmitter plus exactly one added
/// prototype method: `pipe()` (`lib/internal/streams/legacy.js`). It has no
/// `_readableState`/`_writableState`/etc, so — unlike the stream-state
/// inits above — there is no option-driven state to seed here either; this
/// is `js_event_emitter_subclass_init` plus the one extra method. `ns_pipe2`
/// is the same generic, receiver-keyed pipe implementation
/// Readable/Duplex/Transform install (`readable_methods`/
/// `duplex_methods` in `node_stream_readwrite.rs` /
/// `node_stream_duplex_methods.rs`); it drives itself entirely off `on`/
/// `emit` on the source and destination, so it works unmodified on a plain
/// EventEmitter-shaped receiver that never went through a stream
/// constructor.
#[no_mangle]
pub extern "C" fn js_node_stream_legacy_subclass_init(this: f64) -> f64 {
    let raw = raw_ptr_from_value(this);
    if raw == 0 {
        return this;
    }
    if !matches!(unsafe { gc_type_for_ptr(raw) }, Some(crate::gc_object_type_pat!())) {
        return this;
    }
    let obj = raw as *mut ObjectHeader;
    let mut methods: Vec<(&str, StubFn)> = emitter_methods().to_vec();
    methods.push((
        "pipe",
        crate::fn_info!(export crate::node_stream::ns_pipe2, 2; with_declared(2)),
    ));
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
    // A string `options` is the resource name; only an options object carries
    // `captureRejections` for the EventEmitter half.
    let emitter_options = if JSValue::from_bits(options.to_bits()).is_any_string() {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    } else {
        options_handle.get_nanbox_f64()
    };
    js_event_emitter_subclass_init(this_handle.get_nanbox_f64(), emitter_options);

    let this = this_handle.get_nanbox_f64();
    let raw = raw_ptr_from_value(this);
    if raw == 0 || !matches!(unsafe { gc_type_for_ptr(raw) }, Some(crate::gc_object_type_pat!())) {
        return this;
    }
    let options = options_handle.get_nanbox_f64();
    let options_value = JSValue::from_bits(options.to_bits());
    let mut name = if options_value.is_any_string() {
        options
    } else {
        crate::runtime_state_key!(b"name").read_value(options_handle.get_nanbox_f64())
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
        crate::async_hook_slots::js_async_resource_new(name_handle.get_nanbox_f64(), async_options);
    // #10926: `resource` is the public AsyncResource OBJECT now -- ordinary and
    // movable, where it used to be a never-freed `Box` -- and this frame is its
    // only holder until it is stored below, while the hidden key allocates.
    // Root it, and re-read `this` and the resource after that allocation.
    let resource_handle = scope.root_nanbox_f64(resource);
    crate::async_hook_slots::js_async_resource_set_event_emitter(
        raw_ptr_from_value(resource_handle.get_nanbox_f64()) as i64,
        raw_ptr_from_value(this_handle.get_nanbox_f64()) as i64,
    );
    let key = scope.root_string_ptr(hidden_key(EVENT_EMITTER_ASYNC_RESOURCE_KEY));
    unsafe {
        key.with_const_ptr::<crate::StringHeader, _>(|key| {
            crate::object::js_object_set_field_by_name(
                raw_ptr_from_value(this_handle.get_nanbox_f64()) as *mut ObjectHeader,
                key,
                resource_handle.get_nanbox_f64(),
            )
        });
        install_event_emitter_async_resource_instance_methods(
            raw_ptr_from_value(this_handle.get_nanbox_f64()) as *mut ObjectHeader,
            this_handle.get_nanbox_f64(),
        );
    }
    this_handle.get_nanbox_f64()
}

/// node's `Writable` constructor body on an allocated object.
pub(crate) fn init_writable_in_place(
    this: f64,
    opts: f64,
    how: StreamInit,
    provided: Option<super::birth_memo::Input>,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(this);
    let opts = scope.root_nanbox_f64(opts);
    let t = &this;
    let o = &opts;
    let birth = provided.or_else(|| {
        if how == StreamInit::Subclass {
            super::birth_memo::begin_existing(
                t.get_nanbox_f64(),
                o.get_nanbox_f64(),
                super::birth::Kind::Writable,
            )
        } else {
            None
        }
    });
    if get_hidden_value(t.get_nanbox_f64(), crate::runtime_state_key!(b"_events")).is_none() {
        let events = scope.root_nanbox_f64(new_events_object());
        stream_birth_field!(
            t.get_nanbox_f64(),
            crate::runtime_state_key!(b"_events"),
            events.get_nanbox_f64()
        );
    }
    install_stream_state_layout(t.get_nanbox_f64());
    init_lifecycle_state(&this, &opts);
    init_writable_state(&this, &opts, false);
    if let Some(write) = write_callback_from_options(&opts) {
        let write = scope.root_nanbox_f64(write);
        stream_birth_field!(
            t.get_nanbox_f64(),
            crate::runtime_state_key!(b"_write"),
            write.get_nanbox_f64()
        );
        set_hidden_value(
            t.get_nanbox_f64(),
            hidden_write_key(),
            write.get_nanbox_f64(),
        );
    } else if how == StreamInit::Subclass {
        if let Some(write) = subclass_hook(t.get_nanbox_f64(), crate::runtime_state_key!(b"_write"))
        {
            let write = scope.root_nanbox_f64(write);
            set_hidden_value(
                t.get_nanbox_f64(),
                hidden_write_key(),
                write.get_nanbox_f64(),
            );
        }
    }
    if let Some(writev) = writev_callback_from_options(&opts) {
        let writev = scope.root_nanbox_f64(writev);
        stream_birth_field!(
            t.get_nanbox_f64(),
            crate::runtime_state_key!(b"_writev"),
            writev.get_nanbox_f64()
        );
        set_hidden_value(
            t.get_nanbox_f64(),
            hidden_writev_key(),
            writev.get_nanbox_f64(),
        );
    } else if how == StreamInit::Subclass {
        if let Some(writev) =
            subclass_hook(t.get_nanbox_f64(), crate::runtime_state_key!(b"_writev"))
        {
            let writev = scope.root_nanbox_f64(writev);
            set_hidden_value(
                t.get_nanbox_f64(),
                hidden_writev_key(),
                writev.get_nanbox_f64(),
            );
        }
    }
    install_common_lifecycle_callbacks(&this, &opts);
    install_writable_lifecycle_callbacks(&this, &opts);
    invoke_construct_callback(&this, &opts, birth);
    if provided.is_none() && how == StreamInit::Subclass {
        super::birth_memo::finish_existing(t.get_nanbox_f64(), birth, super::birth::Kind::Writable);
    }
}

#[no_mangle]
pub extern "C" fn js_node_stream_writable_new(opts: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let opts = scope.root_nanbox_f64(opts);
    let (writable, proof) = proto_methods::alloc_stream_instance(super::birth::Kind::Writable);
    let writable = scope.root_nanbox_f64(writable);
    init_writable_in_place(
        writable.get_nanbox_f64(),
        opts.get_nanbox_f64(),
        StreamInit::Direct,
        proof,
    );
    writable.get_nanbox_f64()
}

#[no_mangle]
pub extern "C" fn js_node_stream_writable_subclass_init(this: f64, opts: f64) -> f64 {
    if object_ptr_from_value(this).is_none() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(this);
    init_writable_in_place(this.get_nanbox_f64(), opts, StreamInit::Subclass, None);
    this.get_nanbox_f64()
}

/// node's `Duplex` constructor body on an allocated object.
pub(crate) fn init_duplex_in_place(
    this: f64,
    opts: f64,
    how: StreamInit,
    provided: Option<super::birth_memo::Input>,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(this);
    let opts = scope.root_nanbox_f64(opts);
    let t = &this;
    let o = &opts;
    let birth = provided.or_else(|| {
        if how == StreamInit::Subclass {
            super::birth_memo::begin_existing(
                t.get_nanbox_f64(),
                o.get_nanbox_f64(),
                super::birth::Kind::Duplex,
            )
        } else {
            None
        }
    });
    if get_hidden_value(t.get_nanbox_f64(), crate::runtime_state_key!(b"_events")).is_none() {
        let events = scope.root_nanbox_f64(new_events_object());
        stream_birth_field!(
            t.get_nanbox_f64(),
            crate::runtime_state_key!(b"_events"),
            events.get_nanbox_f64()
        );
    }
    install_stream_state_layout(t.get_nanbox_f64());
    init_lifecycle_state(&this, &opts);
    init_readable_state(&this, &opts, true);
    // `_readableState` assignment can invoke an inherited setter and expose
    // this receiver. After that publication, writable fields must honor any
    // descriptors the setter installed, even for a direct Duplex.
    init_writable_state(&this, &opts, true);
    init_duplex_state(&this, &opts);
    let custom_sink = || {
        set_hidden_value(
            t.get_nanbox_f64(),
            Slot::WritableCustomSink,
            f64::from_bits(TAG_TRUE),
        )
    };
    if let Some(read) = read_callback_from_options(&opts) {
        let read = scope.root_nanbox_f64(read);
        stream_birth_field!(
            t.get_nanbox_f64(),
            crate::runtime_state_key!(b"_read"),
            read.get_nanbox_f64()
        );
        set_hidden_value(t.get_nanbox_f64(), hidden_read_key(), read.get_nanbox_f64());
    } else if how == StreamInit::Subclass {
        if let Some(read) = subclass_hook(t.get_nanbox_f64(), crate::runtime_state_key!(b"_read")) {
            let read = scope.root_nanbox_f64(read);
            set_hidden_value(t.get_nanbox_f64(), hidden_read_key(), read.get_nanbox_f64());
        }
    }
    if let Some(write) = write_callback_from_options(&opts) {
        let write = scope.root_nanbox_f64(write);
        stream_birth_field!(
            t.get_nanbox_f64(),
            crate::runtime_state_key!(b"_write"),
            write.get_nanbox_f64()
        );
        set_hidden_value(
            t.get_nanbox_f64(),
            hidden_write_key(),
            write.get_nanbox_f64(),
        );
        custom_sink();
    } else if how == StreamInit::Subclass {
        if let Some(write) = subclass_hook(t.get_nanbox_f64(), crate::runtime_state_key!(b"_write"))
        {
            let write = scope.root_nanbox_f64(write);
            set_hidden_value(
                t.get_nanbox_f64(),
                hidden_write_key(),
                write.get_nanbox_f64(),
            );
            custom_sink();
        }
    }
    if let Some(writev) = writev_callback_from_options(&opts) {
        let writev = scope.root_nanbox_f64(writev);
        stream_birth_field!(
            t.get_nanbox_f64(),
            crate::runtime_state_key!(b"_writev"),
            writev.get_nanbox_f64()
        );
        set_hidden_value(
            t.get_nanbox_f64(),
            hidden_writev_key(),
            writev.get_nanbox_f64(),
        );
        custom_sink();
    } else if how == StreamInit::Subclass {
        if let Some(writev) =
            subclass_hook(t.get_nanbox_f64(), crate::runtime_state_key!(b"_writev"))
        {
            let writev = scope.root_nanbox_f64(writev);
            set_hidden_value(
                t.get_nanbox_f64(),
                hidden_writev_key(),
                writev.get_nanbox_f64(),
            );
            custom_sink();
        }
    }
    #[cfg(test)]
    if crate::node_stream::native_hooks::stream_sabotage("own_methods") {
        // The pre-G1 shape: every method an own property of the instance.
        if let Some(obj) = object_ptr_from_value(t.get_nanbox_f64()) {
            let methods: Vec<(&'static str, StubFn)> = readable_methods()
                .iter()
                .chain(writable_methods().iter())
                .copied()
                .collect();
            install_methods_on_existing_object(obj, t.get_nanbox_f64(), &methods, &["_write"]);
        }
    }
    install_common_lifecycle_callbacks(&this, &opts);
    install_writable_lifecycle_callbacks(&this, &opts);
    invoke_construct_callback(&this, &opts, birth);
    if provided.is_none() && how == StreamInit::Subclass {
        super::birth_memo::finish_existing(t.get_nanbox_f64(), birth, super::birth::Kind::Duplex);
    }
}

#[no_mangle]
pub extern "C" fn js_node_stream_duplex_new(opts: f64) -> f64 {
    new_duplex_kind(opts, DuplexKind::Duplex)
}

#[no_mangle]
pub extern "C" fn js_node_stream_duplex_subclass_init(this: f64, opts: f64) -> f64 {
    if object_ptr_from_value(this).is_none() {
        return this;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(this);
    init_duplex_in_place(this.get_nanbox_f64(), opts, StreamInit::Subclass, None);
    this.get_nanbox_f64()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DuplexKind {
    Duplex,
    Transform,
    PassThrough,
}

fn new_duplex_kind(opts: f64, kind: DuplexKind) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let opts = scope.root_nanbox_f64(opts);
    let (stream, proof) = proto_methods::alloc_stream_instance(match kind {
        DuplexKind::Duplex => super::birth::Kind::Duplex,
        DuplexKind::Transform => super::birth::Kind::Transform,
        DuplexKind::PassThrough => super::birth::Kind::PassThrough,
    });
    let stream = scope.root_nanbox_f64(stream);
    match kind {
        DuplexKind::Duplex => init_duplex_in_place(
            stream.get_nanbox_f64(),
            opts.get_nanbox_f64(),
            StreamInit::Direct,
            proof,
        ),
        DuplexKind::Transform => init_transform_kind(
            stream.get_nanbox_f64(),
            opts.get_nanbox_f64(),
            StreamInit::Direct,
            false,
            proof,
        ),
        DuplexKind::PassThrough => init_transform_kind(
            stream.get_nanbox_f64(),
            opts.get_nanbox_f64(),
            StreamInit::Direct,
            true,
            proof,
        ),
    }
    stream.get_nanbox_f64()
}

/// node's `Transform` constructor body on an allocated object: the Duplex
/// state, then the transform hooks (an option, or a subclass override found
/// on the chain). `passthrough` is `PassThrough`'s identity transform when no
/// hook is given.
fn init_transform_kind(
    this: f64,
    opts: f64,
    how: StreamInit,
    passthrough: bool,
    provided: Option<super::birth_memo::Input>,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(this);
    let opts = scope.root_nanbox_f64(opts);
    let hwm = super::normalized_hwm(
        &opts,
        crate::runtime_state_key!(b"readableHighWaterMark"),
        "options.readableHighWaterMark",
        true,
        false,
    );
    let opts = if hwm == 0.0 {
        let normalized = scope.root_nanbox_f64(unsafe {
            crate::object::js_object_spread_fresh(
                f64::from_bits(TAG_UNDEFINED),
                opts.get_nanbox_f64(),
            )
        });
        let writable_hwm =
            crate::runtime_state_key!(b"writableHighWaterMark").read_value(opts.get_nanbox_f64());
        let writable_hwm = if crate::value::js_is_truthy(writable_hwm) != 0 {
            writable_hwm
        } else {
            0.0
        };
        for (key, value) in [
            (
                crate::runtime_state_key!(b"highWaterMark"),
                f64::from_bits(TAG_NULL),
            ),
            (crate::runtime_state_key!(b"readableHighWaterMark"), 0.0),
            (
                crate::runtime_state_key!(b"writableHighWaterMark"),
                writable_hwm,
            ),
        ] {
            set_visible_own_value(normalized.get_nanbox_f64(), key, value);
        }
        normalized
    } else {
        opts
    };
    let kind = if passthrough {
        super::birth::Kind::PassThrough
    } else {
        super::birth::Kind::Transform
    };
    let birth = provided.or_else(|| {
        if how == StreamInit::Subclass {
            super::birth_memo::begin_existing(this.get_nanbox_f64(), opts.get_nanbox_f64(), kind)
        } else {
            None
        }
    });
    let t = &this;
    let o = &opts;
    init_duplex_in_place(t.get_nanbox_f64(), o.get_nanbox_f64(), how, birth);
    if crate::node_stream::native_hooks::hooks_of(t.get_nanbox_f64()).is_some() {
        crate::node_stream::native_hooks::init_runner_slots(t.get_nanbox_f64());
    }
    let subclass_transform = match how {
        StreamInit::Subclass => {
            match subclass_hook(t.get_nanbox_f64(), crate::runtime_state_key!(b"_transform")) {
                Some(v) => Some(scope.root_nanbox_f64(v)),
                None => None,
            }
        }
        StreamInit::Direct => None,
    };
    let subclass_flush = match how {
        StreamInit::Subclass => {
            match subclass_hook(t.get_nanbox_f64(), crate::runtime_state_key!(b"_flush")) {
                Some(v) => Some(scope.root_nanbox_f64(v)),
                None => None,
            }
        }
        StreamInit::Direct => None,
    };
    if let Some(callback) = transform_callback_from_options(&opts) {
        let callback = scope.root_nanbox_f64(callback);
        stream_birth_field!(
            t.get_nanbox_f64(),
            crate::runtime_state_key!(b"_transform"),
            callback.get_nanbox_f64()
        );
        set_hidden_value(
            t.get_nanbox_f64(),
            hidden_transform_callback_key(),
            callback.get_nanbox_f64(),
        );
    } else if let Some(callback) = &subclass_transform {
        set_hidden_value(
            t.get_nanbox_f64(),
            hidden_transform_callback_key(),
            callback.get_nanbox_f64(),
        );
    }
    if let Some(flush) = transform_flush_from_options(&opts) {
        let flush = scope.root_nanbox_f64(flush);
        stream_birth_field!(
            t.get_nanbox_f64(),
            crate::runtime_state_key!(b"_flush"),
            flush.get_nanbox_f64()
        );
        set_hidden_value(
            t.get_nanbox_f64(),
            hidden_transform_flush_key(),
            flush.get_nanbox_f64(),
        );
    } else if let Some(flush) = &subclass_flush {
        set_hidden_value(
            t.get_nanbox_f64(),
            hidden_transform_flush_key(),
            flush.get_nanbox_f64(),
        );
    }
    if crate::node_stream::native_hooks::hooks_of(t.get_nanbox_f64())
        .is_some_and(|h| h.timing == crate::node_stream::native_hooks::StepTiming::DEFERRED)
    {
        // A binding's inherited final/flush bodies participate in the same
        // lifecycle as user hooks. In particular a no-op _final completes
        // writable finish before the deferred _flush output is consumed.
        for (name, slot) in [
            (
                crate::runtime_state_key!(b"_flush"),
                hidden_transform_flush_key(),
            ),
            (
                crate::runtime_state_key!(b"_final"),
                hidden_writable_final_key(),
            ),
        ] {
            if get_hidden_value(t.get_nanbox_f64(), slot).is_none() {
                let hook = scope.root_nanbox_f64(unsafe {
                    name.read_object(object_ptr_from_value(t.get_nanbox_f64()).unwrap())
                });
                // `Transform.prototype._final` is node's, not the binding's.
                if crate::node_stream::user_hook_value(hook.get_nanbox_f64()).is_some() {
                    set_hidden_value(t.get_nanbox_f64(), slot, hook.get_nanbox_f64());
                }
            }
        }
    }
    mark_transform_stream(t.get_nanbox_f64());
    if passthrough && transform_hidden_callback(t.get_nanbox_f64()).is_none() {
        set_hidden_value(
            t.get_nanbox_f64(),
            hidden_transform_passthrough_key(),
            f64::from_bits(TAG_TRUE),
        );
    }
    let prefinish = js_closure_alloc(crate::fn_info!(transform_prefinish, 0), 0);
    add_stream_listener_for_event(
        t.get_nanbox_f64(),
        literal_string_value(b"prefinish"),
        box_pointer(prefinish.cast()),
    );
    if provided.is_none() && how == StreamInit::Subclass {
        super::birth_memo::finish_existing(this.get_nanbox_f64(), birth, kind);
    }
}

/// `Transform`'s constructor body for a native-payload stream family (zlib,
/// crypto, the test rot13 family), on an object its family allocated with its
/// own prototype (`native_payload::alloc_stream`). The codec is reached through
/// the payload's hooks; a JS `_transform`/`_flush` override on a subclass is
/// still captured here and takes precedence (`native_hooks`).
pub fn init_transform_in_place(stream: f64, opts: f64) {
    if crate::node_stream::native_hooks::hooks_of(stream).is_some_and(|hooks| hooks.lazy) {
        #[cfg(test)]
        if crate::node_stream::native_hooks::stream_sabotage("eager_lazy_init") {
            init_transform_kind(stream, opts, StreamInit::Subclass, false, None);
            return;
        }
        set_hidden_value(stream, Slot::NativeStreamOptions, opts);
        return;
    }
    init_transform_kind(stream, opts, StreamInit::Subclass, false, None);
}

/// First stream use of a LazyTransform. The ordinary stream flags are the
/// initialization proof; there is no extra latch or native state machine.
pub(crate) fn ensure_lazy_stream(stream: f64) -> f64 {
    if !crate::node_stream::native_hooks::hooks_of(stream).is_some_and(|hooks| hooks.lazy)
        || get_hidden_value(stream, hidden_readable_flag_key()).is_some()
    {
        return stream;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    let opts = scope.root_nanbox_f64(
        get_hidden_value(stream.get_nanbox_f64(), Slot::NativeStreamOptions)
            .unwrap_or(f64::from_bits(TAG_UNDEFINED)),
    );
    init_transform_kind(
        stream.get_nanbox_f64(),
        opts.get_nanbox_f64(),
        StreamInit::Subclass,
        false,
        None,
    );
    stream.get_nanbox_f64()
}

/// `Writable`'s constructor body for a native-payload Writable family
/// (`Sign`/`Verify`).
pub fn init_writable_payload_in_place(stream: f64, opts: f64) {
    init_writable_in_place(stream, opts, StreamInit::Subclass, None);
}

#[no_mangle]
pub extern "C" fn js_node_stream_transform_new(opts: f64) -> f64 {
    new_duplex_kind(opts, DuplexKind::Transform)
}

#[no_mangle]
pub extern "C" fn js_node_stream_transform_subclass_init(this: f64, opts: f64) -> f64 {
    if object_ptr_from_value(this).is_none() {
        return this;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(this);
    init_transform_kind(
        this.get_nanbox_f64(),
        opts,
        StreamInit::Subclass,
        false,
        None,
    );
    this.get_nanbox_f64()
}

#[no_mangle]
pub extern "C" fn js_node_stream_passthrough_new(opts: f64) -> f64 {
    new_duplex_kind(opts, DuplexKind::PassThrough)
}

/// Initialize `class X extends PassThrough` without replacing the derived
/// instance. A subclass-provided `_transform` wins; otherwise retain
/// PassThrough's identity transform instead of falling into Transform's
/// missing-method error.
#[no_mangle]
pub extern "C" fn js_node_stream_passthrough_subclass_init(this: f64, opts: f64) -> f64 {
    if object_ptr_from_value(this).is_none() {
        return this;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(this);
    init_transform_kind(
        this.get_nanbox_f64(),
        opts,
        StreamInit::Subclass,
        true,
        None,
    );
    this.get_nanbox_f64()
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
    // Normalizing can run the iterable's own code, so hold everything across
    // it in handles (#11828).
    let scope = crate::gc::RuntimeHandleScope::new();
    let iterable = scope.root_nanbox_f64(iterable);
    let options = readable_from_options(opts);
    let readable = scope.root_nanbox_f64(js_node_stream_readable_new(options));
    let read = scope.root_raw_mut_ptr(js_closure_alloc(
        crate::fn_info!(from_read, 1; with_declared(0)),
        0,
    ));
    stream_birth_field!(
        readable.get_nanbox_f64(),
        crate::runtime_state_key!(b"_read"),
        box_pointer(read.get_raw_mut_ptr::<ClosureHeader>().cast())
    );
    let destroy = scope.root_raw_mut_ptr(js_closure_alloc(crate::fn_info!(from_destroy, 2), 0));
    stream_birth_field!(
        readable.get_nanbox_f64(),
        crate::runtime_state_key!(b"_destroy"),
        box_pointer(destroy.get_raw_mut_ptr::<ClosureHeader>().cast())
    );
    set_hidden_value(
        readable.get_nanbox_f64(),
        hidden_read_key(),
        box_pointer(read.get_raw_mut_ptr::<ClosureHeader>().cast()),
    );
    set_hidden_value(
        readable.get_nanbox_f64(),
        STREAM_DESTROY_KEY,
        box_pointer(destroy.get_raw_mut_ptr::<ClosureHeader>().cast()),
    );
    set_hidden_value(
        readable.get_nanbox_f64(),
        hidden_default_read_error_key(),
        f64::from_bits(TAG_FALSE),
    );
    if raw_ptr_from_value(readable.get_nanbox_f64()) >= 0x10000 {
        // Armed in a C trampoline frame (#9305); both continuations run
        // after the trap is popped, as before.
        match crate::exception::catch_js_throw(|| {
            normalize_readable_from_input(iterable.get_nanbox_f64())
        }) {
            Ok(normalized) => {
                let chunks = scope.root_nanbox_f64(normalized.chunks);
                let source_iterator = match normalized.source_iterator {
                    Some(source_iterator) => Some(scope.root_nanbox_f64(source_iterator)),
                    None => None,
                };
                set_hidden_value(
                    readable.get_nanbox_f64(),
                    hidden_chunks_key(),
                    chunks.get_nanbox_f64(),
                );
                set_hidden_value(
                    readable.get_nanbox_f64(),
                    Slot::ReadableLivePush,
                    f64::from_bits(TAG_FALSE),
                );
                initialize_readable_from_buffered_length(
                    readable.get_nanbox_f64(),
                    chunks.get_nanbox_f64(),
                );
                if let Some(source_iterator) = source_iterator {
                    set_hidden_value(
                        readable.get_nanbox_f64(),
                        READABLE_SOURCE_ITERATOR_KEY,
                        source_iterator.get_nanbox_f64(),
                    );
                }
            }
            Err(err) => {
                destroy_stream(readable.get_nanbox_f64(), err);
            }
        }
    }
    readable.get_nanbox_f64()
}

extern "C" fn transform_prefinish(_: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
    let stream = this.as_f64();
    if !finish_transform_stream(stream, None) {
        schedule_readable_end(stream);
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn from_read(_: *const ClosureHeader, this: crate::closure::JsThis, _: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(this.as_f64());
    if get_hidden_value(stream.get_nanbox_f64(), READABLE_SOURCE_ITERATOR_KEY).is_some() {
        if !readable_chunks_nonempty(stream.get_nanbox_f64()) {
            super::super::readable_from_iterator::pull(stream.get_nanbox_f64());
        }
    } else {
        // The existing finite-input driver owns EOF for a normalized array.
        set_hidden_value(
            stream.get_nanbox_f64(),
            hidden_readable_live_push_key(),
            f64::from_bits(TAG_FALSE),
        );
        if !readable_chunks_nonempty(stream.get_nanbox_f64()) {
            mark_stream_ended(stream.get_nanbox_f64());
            schedule_readable_end(stream.get_nanbox_f64());
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn from_destroy(
    _: *const ClosureHeader,
    this: crate::closure::JsThis,
    err: f64,
    callback: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(this.as_f64());
    let err = scope.root_nanbox_f64(err);
    let callback = scope.root_nanbox_f64(callback);
    super::super::async_iterator::call_source_iterator_return(stream.get_nanbox_f64());
    set_hidden_value(
        stream.get_nanbox_f64(),
        READABLE_SOURCE_ITERATOR_KEY,
        f64::from_bits(TAG_UNDEFINED),
    );
    unsafe {
        crate::closure::native_call_value_this(
            callback.get_nanbox_f64(),
            crate::closure::JsThis::from_f64(stream.get_nanbox_f64()),
            [err.get_nanbox_f64()].as_ptr(),
            1,
        );
    }
    f64::from_bits(TAG_UNDEFINED)
}
