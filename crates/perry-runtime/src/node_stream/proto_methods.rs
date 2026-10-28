//! G1 (STREAM-PAYLOAD-DESIGN): the classic stream methods live on the stream
//! prototypes, as in node, not as bound closures on every instance.
//!
//! `Readable.prototype`, `Writable.prototype`, `Duplex.prototype` and
//! `Transform.prototype` are born on their static layouts
//! (`object::runtime_layouts::node_prototypes`: exactly node's own keys, order
//! and attributes) once per realm, when the constructor's prototype is built
//! (`object::native_module_stream::attach_stream_constructor_prototype`), and
//! sealed. Each method is one IMPLICIT_THIS closure (capture slot 0 holds
//! `undefined`, so `this_value` reads the call-site receiver) shared by every
//! instance: `Transform`/`PassThrough` and their subclasses inherit them
//! through the prototype chain (`Transform.prototype -> Duplex.prototype ->
//! Readable.prototype -> Stream.prototype -> EventEmitter.prototype`), a
//! source-compiled `class X extends Readable` through its class prototype's
//! parent edge, and a payload family (zlib, crypto) through its family
//! prototype. A subclass override shadows the inherited method by ordinary
//! lookup, and `super.m()` reaches the base body on the chain.
//!
//! The hook methods node defines on the prototypes (`_read`, `_write`,
//! `_destroy`, `_final`, `_transform`, ...) are runtime builtins
//! (`FN_BUILTIN`): the runtime's own stream machinery never takes one for a
//! user hook ([`is_builtin_hook`]); a program that calls one directly
//! (`super._read()`, `Writable.prototype._write.call(...)`) gets node's body.
//!
//! Instances own Node's enumerable state and optional option-supplied hooks.
//! The private record adds no string keys, so reflection and JSON use the
//! same ordinary shape and own-property order.

use super::*;
use crate::object::runtime_layouts as layouts;
use crate::object::ObjectHeader;

/// Which stream prototype.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum StreamProto {
    Readable,
    Writable,
    Duplex,
    Transform,
}

impl StreamProto {
    fn layout(self) -> *const crate::object::layout_desc::PerryLayoutDesc {
        match self {
            StreamProto::Readable => std::ptr::addr_of!(layouts::READABLE_PROTOTYPE),
            StreamProto::Writable => std::ptr::addr_of!(layouts::WRITABLE_PROTOTYPE),
            StreamProto::Duplex => std::ptr::addr_of!(layouts::DUPLEX_PROTOTYPE),
            StreamProto::Transform => std::ptr::addr_of!(layouts::TRANSFORM_PROTOTYPE),
        }
    }

    fn serial(self) -> u64 {
        use crate::closure::shape as s;
        match self {
            StreamProto::Readable => s::INTRINSIC_SERIAL_READABLE_PROTOTYPE,
            StreamProto::Writable => s::INTRINSIC_SERIAL_WRITABLE_PROTOTYPE,
            StreamProto::Duplex => s::INTRINSIC_SERIAL_DUPLEX_PROTOTYPE,
            StreamProto::Transform => s::INTRINSIC_SERIAL_TRANSFORM_PROTOTYPE,
        }
    }

    /// The stream prototype that is this one's [[Prototype]] (its layout
    /// names it through the parent's identity word); `None`: the legacy
    /// `Stream.prototype`.
    fn parent(self) -> Option<StreamProto> {
        match self {
            StreamProto::Readable | StreamProto::Writable => None,
            StreamProto::Duplex => Some(StreamProto::Readable),
            StreamProto::Transform => Some(StreamProto::Duplex),
        }
    }

    fn export_name(self) -> &'static str {
        match self {
            StreamProto::Readable => "Readable",
            StreamProto::Writable => "Writable",
            StreamProto::Duplex => "Duplex",
            StreamProto::Transform => "Transform",
        }
    }
}

/// `kind`'s prototype, built now if this agent has none yet. Minting a stream
/// export attaches the whole module (`Stream.Readable = ...`), so a child's
/// prototype can be asked for while its parent's constructor is still being
/// attached: the parent is then built here, on the constructor that already
/// exists, and the outer attach finds it built ([`build_stream_prototype`]).
fn ensure_stream_prototype(kind: StreamProto) -> f64 {
    if let Some(proto) = layouts::node_prototype_of_serial(kind.serial()) {
        return crate::value::js_nanbox_pointer(proto as i64);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let ctor = scope.root_nanbox_f64(crate::object::bound_native_callable_export_value(
        "stream",
        kind.export_name(),
    ));
    if let Some(proto) = layouts::node_prototype_of_serial(kind.serial()) {
        return crate::value::js_nanbox_pointer(proto as i64);
    }
    let proto = scope.root_nanbox_f64(build_stream_prototype(ctor.get_nanbox_f64(), kind));
    if JSValue::from_bits(ctor.get_nanbox_u64()).is_pointer() {
        crate::closure::closure_set_dynamic_prop(
            (ctor.get_nanbox_u64() & crate::value::POINTER_MASK) as usize,
            "prototype",
            proto.get_nanbox_f64(),
        );
    }
    proto.get_nanbox_f64()
}

/// The hook methods node's stream prototypes carry (`_read`, `_write`,
/// `_destroy`, `_final`, ...): the bodies [`is_builtin_hook`] recognises.
static NODE_PROTOTYPE_HOOKS: [&crate::closure::JsFunctionInfo; 13] = [
    &NS_BASE_READ_INFO,
    &NS_BASE_WRITE_INFO,
    &NS_BASE_DESTROY_INFO,
    &NS_UNDESTROY_INFO,
    &NS_CANNOT_PIPE_INFO,
    &NS_REJECTION_DESTROY_INFO,
    &NS_TO_ASYNC_STREAMABLE_INFO,
    &NS_TRANSFORM3_INFO,
    &NS_TRANSFORM_WRITE_INFO,
    &NS_TRANSFORM_READ_INFO,
    &NS_TRANSFORM_FINAL_INFO,
    &NS_LEGACY_PIPE_INFO,
    &NS_PASSTHROUGH_TRANSFORM_INFO,
];

/// Is `value` one of node's prototype hook methods, which the stream
/// machinery treats as "no user hook" and answers natively? A binding's own
/// builtin hook (zlib, crypto) is a hook like a user's.
pub(crate) fn is_builtin_hook(value: f64) -> bool {
    if !is_callable_value(value) {
        return false;
    }
    let raw = raw_ptr_from_value(value);
    if raw < 0x10000 || !crate::closure::is_closure_ptr(raw) {
        return false;
    }
    let info = unsafe { (*(raw as *const ClosureHeader)).info };
    NODE_PROTOTYPE_HOOKS
        .iter()
        .any(|hook| std::ptr::eq(*hook, info))
}

/// `value` when it is a callable that is not one of node's prototype hooks.
pub(crate) fn user_hook_value(value: f64) -> Option<f64> {
    (is_callable_value(value) && !is_builtin_hook(value)).then_some(value)
}

/// One receiver-from-`this` closure of `info`.
fn proto_closure(
    scope: &crate::gc::RuntimeHandleScope,
    info: StubFn,
) -> crate::gc::RuntimeHandle<'_> {
    let closure = crate::closure::js_closure_alloc(info, 1);
    crate::closure::js_closure_set_capture_ptr(closure, 0, crate::value::TAG_UNDEFINED as i64);
    scope.root_nanbox_f64(box_pointer(closure as *const u8))
}

/// Build `kind`'s prototype for `constructor`: born on its layout, every slot
/// filled, sealed. Returns it NaN-boxed.
pub(crate) fn build_stream_prototype(constructor: f64, kind: StreamProto) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let constructor = scope.root_nanbox_f64(constructor);
    // The parent's prototype exists (and its identity word names it) before
    // this one is born on a layout naming it.
    match kind.parent() {
        Some(parent) => {
            let _ = ensure_stream_prototype(parent);
        }
        None => {
            if layouts::node_prototype_of_serial(
                crate::closure::shape::INTRINSIC_SERIAL_STREAM_PROTOTYPE,
            )
            .is_none()
            {
                let _ = stream_prototype_value("Stream");
            }
        }
    }
    // Ensuring the parent attaches the module, which may have built this
    // prototype already: one prototype per family and agent.
    if let Some(proto) = layouts::node_prototype_of_serial(kind.serial()) {
        return crate::value::js_nanbox_pointer(proto as i64);
    }
    // SAFETY: a node prototype descriptor of this runtime.
    let layout = unsafe { layouts::NodePrototype::of(kind.layout()) };
    let proto = scope.root_raw_mut_ptr(unsafe { layout.birth() });
    let p = || proto.get_raw_mut_ptr::<ObjectHeader>();
    // SAFETY (every fill below): the fresh prototype born on `layout`, not yet
    // reachable by program code.
    unsafe { layout.data(p(), b"constructor", constructor.get_nanbox_f64()) };
    let readable = readable_methods();
    let writable = writable_methods();
    let info_of = |name: &str| -> StubFn {
        readable
            .iter()
            .chain(writable.iter())
            .find(|(n, _)| *n == name)
            .map(|(_, info)| *info)
            .expect("stream method table names every prototype method")
    };
    let fill = |name: &str, value: f64| unsafe { layout.data(p(), name.as_bytes(), value) };
    let method = |name: &str| {
        let value = proto_closure(&scope, info_of(name));
        fill(name, value.get_nanbox_f64());
        value.get_nanbox_f64()
    };
    let builtin = |name: &str, info: StubFn| {
        let value = proto_closure(&scope, info);
        fill(name, value.get_nanbox_f64());
    };
    let symbol = |atom: *const crate::object::layout_desc::StaticAtom, info: StubFn| {
        let value = proto_closure(&scope, info);
        unsafe { layout.symbol_data(p(), atom, value.get_nanbox_f64()) };
    };
    match kind {
        StreamProto::Readable => {
            for name in [
                "destroy",
                "push",
                "unshift",
                "isPaused",
                "setEncoding",
                "read",
                "pipe",
                "unpipe",
                "removeListener",
                "off",
                "removeAllListeners",
                "resume",
                "pause",
                "wrap",
                "iterator",
                "compose",
                "drop",
                "filter",
                "flatMap",
                "map",
                "take",
                "every",
                "forEach",
                "reduce",
                "toArray",
                "some",
                "find",
            ] {
                method(name);
            }
            // `addListener` IS `on` (node's alias), so both names hold one value.
            let on = method("on");
            fill("addListener", on);
            builtin("_undestroy", &NS_UNDESTROY_INFO);
            builtin("_destroy", &NS_BASE_DESTROY_INFO);
            builtin("_read", &NS_BASE_READ_INFO);
            for (name, prop) in [
                ("readable", Prop::Readable),
                ("readableDidRead", Prop::ReadableDidRead),
                ("readableAborted", Prop::ReadableAborted),
                ("readableHighWaterMark", Prop::ReadableHwm),
                ("readableBuffer", Prop::ReadableBuffer),
                ("readableFlowing", Prop::ReadableFlowing),
                ("readableLength", Prop::ReadableLength),
                ("readableObjectMode", Prop::ReadableObjectMode),
                ("readableEncoding", Prop::ReadableEncoding),
                ("errored", Prop::Errored),
                ("closed", Prop::Closed),
                ("destroyed", Prop::Destroyed),
                ("readableEnded", Prop::ReadableEnded),
            ] {
                install_prop(&layout, p(), name, prop);
            }
            symbol(
                std::ptr::addr_of!(layouts::NP_SYMBOL_NODEJS_REJECTION),
                &NS_REJECTION_DESTROY_INFO,
            );
            symbol(
                std::ptr::addr_of!(layouts::NP_ATOM_SYMBOL_ASYNC_DISPOSE),
                crate::fn_info!(ns_async_dispose, 0; with_declared(0)),
            );
            symbol(
                std::ptr::addr_of!(layouts::NP_ATOM_SYMBOL_ASYNC_ITERATOR),
                crate::fn_info!(super::async_iterator::ns_async_iterator, 0; with_declared(0)),
            );
            symbol(
                std::ptr::addr_of!(layouts::NP_SYMBOL_TO_ASYNC_STREAMABLE),
                &NS_TO_ASYNC_STREAMABLE_INFO,
            );
        }
        StreamProto::Writable => {
            for name in [
                "write",
                "cork",
                "uncork",
                "setDefaultEncoding",
                "end",
                "destroy",
            ] {
                method(name);
            }
            builtin("pipe", &NS_CANNOT_PIPE_INFO);
            builtin("_write", &NS_BASE_WRITE_INFO);
            fill("_writev", f64::from_bits(TAG_NULL));
            builtin("_undestroy", &NS_UNDESTROY_INFO);
            builtin("_destroy", &NS_BASE_DESTROY_INFO);
            for (name, prop) in [
                ("closed", Prop::Closed),
                ("destroyed", Prop::Destroyed),
                ("writable", Prop::Writable),
                ("writableFinished", Prop::WritableFinished),
                ("writableObjectMode", Prop::WritableObjectMode),
                ("writableBuffer", Prop::WritableBuffer),
                ("writableEnded", Prop::WritableEnded),
                ("writableNeedDrain", Prop::WritableNeedDrain),
                ("writableHighWaterMark", Prop::WritableHwm),
                ("writableCorked", Prop::WritableCorked),
                ("writableLength", Prop::WritableLength),
                ("errored", Prop::Errored),
                ("writableAborted", Prop::WritableAborted),
            ] {
                install_prop(&layout, p(), name, prop);
            }
            symbol(
                std::ptr::addr_of!(layouts::NP_SYMBOL_NODEJS_REJECTION),
                &NS_REJECTION_DESTROY_INFO,
            );
            symbol(
                std::ptr::addr_of!(layouts::NP_ATOM_SYMBOL_ASYNC_DISPOSE),
                crate::fn_info!(ns_async_dispose, 0; with_declared(0)),
            );
        }
        // node's Duplex.prototype adds the writable half; the readable half
        // and the emitter methods come from Readable.prototype.
        StreamProto::Duplex => {
            for name in [
                "write",
                "cork",
                "uncork",
                "setDefaultEncoding",
                "end",
                "destroy",
            ] {
                method(name);
            }
            builtin("_write", &NS_BASE_WRITE_INFO);
            fill("_writev", f64::from_bits(TAG_NULL));
            for (name, prop) in [
                ("writable", Prop::Writable),
                ("writableHighWaterMark", Prop::WritableHwm),
                ("writableObjectMode", Prop::WritableObjectMode),
                ("writableBuffer", Prop::WritableBuffer),
                ("writableLength", Prop::WritableLength),
                ("writableFinished", Prop::WritableFinished),
                ("writableCorked", Prop::WritableCorked),
                ("writableEnded", Prop::WritableEnded),
                ("writableNeedDrain", Prop::WritableNeedDrain),
                ("destroyed", Prop::Destroyed),
            ] {
                install_prop(&layout, p(), name, prop);
            }
        }
        StreamProto::Transform => {
            builtin("_final", &NS_TRANSFORM_FINAL_INFO);
            builtin("_transform", &NS_TRANSFORM3_INFO);
            builtin("_write", &NS_TRANSFORM_WRITE_INFO);
            builtin("_read", &NS_TRANSFORM_READ_INFO);
        }
    }
    // SAFETY: populated; program code has not seen it.
    unsafe { layout.seal(p(), kind.serial()) };
    proto.with_mut_ptr::<ObjectHeader, _>(|proto| crate::value::js_nanbox_pointer(proto as i64))
}

/// `PassThrough.prototype` for `constructor`: born on its layout (node's
/// `constructor` and `_transform`), filled, sealed.
pub(crate) fn build_passthrough_prototype(constructor: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let constructor = scope.root_nanbox_f64(constructor);
    let _ = ensure_stream_prototype(StreamProto::Transform);
    let serial = crate::closure::shape::INTRINSIC_SERIAL_PASSTHROUGH_PROTOTYPE;
    if let Some(proto) = layouts::node_prototype_of_serial(serial) {
        return crate::value::js_nanbox_pointer(proto as i64);
    }
    // SAFETY: a node prototype descriptor of this runtime.
    let layout = unsafe { layouts::NodePrototype::of(&raw const layouts::PASSTHROUGH_PROTOTYPE) };
    let proto = scope.root_raw_mut_ptr(unsafe { layout.birth() });
    let p = || proto.get_raw_mut_ptr::<ObjectHeader>();
    let transform = proto_closure(&scope, &NS_PASSTHROUGH_TRANSFORM_INFO);
    // SAFETY: the fresh prototype born on `layout`, not yet reachable by
    // program code; populated before it is sealed.
    unsafe {
        layout.data(p(), b"constructor", constructor.get_nanbox_f64());
        layout.data(p(), b"_transform", transform.get_nanbox_f64());
        layout.seal(p(), serial);
    }
    proto.with_mut_ptr::<ObjectHeader, _>(|proto| crate::value::js_nanbox_pointer(proto as i64))
}

/// `stream.<name>.prototype`, the object a direct `new <name>()` is born on
/// and the one a subclass prototype's parent edge names
/// (`class_registry::state::reserved_native_parent_prototype_bits`).
pub(crate) fn stream_prototype_value(name: &str) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let ctor = scope.root_nanbox_f64(crate::object::bound_native_callable_export_value(
        "stream", name,
    ));
    crate::object::js_function_prototype_value_for_read(ctor.get_nanbox_f64())
}

/// `Object.create(stream.<name>.prototype)`: a direct instance, which owns
/// only the state its init defines.
pub(super) fn alloc_stream_instance(
    kind: super::constructors::birth::Kind,
) -> (f64, Option<super::constructors::birth_memo::Input>) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let (name, native_key) = kind.constructor();
    let ctor = scope.root_nanbox_f64(
        crate::object::native_module::minted_native_callable_export(native_key)
            .unwrap_or_else(|| crate::object::bound_native_callable_export_value("stream", name)),
    );
    let proto = scope.root_nanbox_f64(
        super::constructors::own_prototype_by_bag(ctor.get_nanbox_f64()).unwrap_or_else(|| {
            crate::object::js_function_prototype_value_for_read(ctor.get_nanbox_f64())
        }),
    );
    if !JSValue::from_bits(proto.get_nanbox_u64()).is_pointer() {
        // The writable constructor `.prototype` can be replaced by a
        // primitive. Keep the ordinary fallback prototype while still
        // allocating the data layout required by slot initialization.
        let ordinary = scope.root_nanbox_f64(box_pointer(
            crate::object::js_object_alloc(0, 0) as *const u8
        ));
        proto.set_nanbox_f64(crate::object::js_object_get_prototype_of(
            ordinary.get_nanbox_f64(),
        ));
    }
    // SAFETY: a registered stream instance layout.
    if let Some(obj) = unsafe {
        super::constructors::instance_birth::alloc_on_layout(proto.get_nanbox_f64(), kind.layout())
    } {
        return (box_pointer(obj.cast()), None);
    }
    // A written or relinked chain: the ordinary construction, whose stores
    // are the ordinary `[[Set]]`s.
    let object = scope.root_raw_mut_ptr(crate::object::js_object_alloc_plain(2));
    let stream = object.with_mut_ptr::<crate::object::ObjectHeader, _>(|obj| {
        crate::object::prototype_chain::object_link_created_prototype(
            obj as usize,
            proto.get_nanbox_u64(),
        );
        box_pointer(obj.cast())
    });
    (stream, None)
}

/// Does `value`'s prototype chain reach `stream.<name>.prototype`? (node's
/// ordinary `instanceof`; `Writable` additionally answers for any classic
/// stream with a writable side, as node's `Writable[Symbol.hasInstance]`
/// does for a Duplex.)
pub(crate) fn chain_reaches_stream_prototype(value: f64, name: &str) -> bool {
    let target = stream_prototype_value(name);
    if !JSValue::from_bits(target.to_bits()).is_pointer() {
        return false;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(target);
    let current = scope.root_nanbox_f64(crate::object::js_object_get_prototype_of(value));
    for _ in 0..64 {
        let bits = current.get_nanbox_u64();
        if !JSValue::from_bits(bits).is_pointer() {
            return false;
        }
        if bits == target.get_nanbox_u64() {
            return true;
        }
        current.set_nanbox_f64(crate::object::js_object_get_prototype_of(
            current.get_nanbox_f64(),
        ));
    }
    false
}

// ─────────────────────────────────────────────────────────────────
// The hook methods node defines on the prototypes. Each is a builtin: the
// stream machinery answers a stream without a user hook natively.
// ─────────────────────────────────────────────────────────────────

static NS_BASE_READ_INFO: crate::closure::JsFunctionInfo =
    crate::closure::JsFunctionInfo::of(ns_base_read1 as crate::codegen_abi::JsBody1<ClosureHeader>)
        .with_declared(1)
        .with_flags(crate::closure::FN_BUILTIN);
static NS_BASE_WRITE_INFO: crate::closure::JsFunctionInfo = crate::closure::JsFunctionInfo::of(
    ns_base_write3 as crate::codegen_abi::JsBody3<ClosureHeader>,
)
.with_declared(3)
.with_flags(crate::closure::FN_BUILTIN);
static NS_BASE_DESTROY_INFO: crate::closure::JsFunctionInfo = crate::closure::JsFunctionInfo::of(
    ns_base_destroy2 as crate::codegen_abi::JsBody2<ClosureHeader>,
)
.with_declared(2)
.with_flags(crate::closure::FN_BUILTIN);
static NS_UNDESTROY_INFO: crate::closure::JsFunctionInfo =
    crate::closure::JsFunctionInfo::of(ns_undestroy0 as crate::codegen_abi::JsBody0<ClosureHeader>)
        .with_declared(0)
        .with_flags(crate::closure::FN_BUILTIN);
static NS_CANNOT_PIPE_INFO: crate::closure::JsFunctionInfo = crate::closure::JsFunctionInfo::of(
    ns_cannot_pipe0 as crate::codegen_abi::JsBody0<ClosureHeader>,
)
.with_declared(0)
.with_flags(crate::closure::FN_BUILTIN);
static NS_REJECTION_DESTROY_INFO: crate::closure::JsFunctionInfo =
    crate::closure::JsFunctionInfo::of(
        ns_rejection_destroy1 as crate::codegen_abi::JsBody1<ClosureHeader>,
    )
    .with_declared(1)
    .with_flags(crate::closure::FN_BUILTIN);
static NS_TO_ASYNC_STREAMABLE_INFO: crate::closure::JsFunctionInfo =
    crate::closure::JsFunctionInfo::of(
        ns_to_async_streamable0 as crate::codegen_abi::JsBody0<ClosureHeader>,
    )
    .with_declared(0)
    .with_flags(crate::closure::FN_BUILTIN);
static NS_TRANSFORM3_INFO: crate::closure::JsFunctionInfo =
    crate::closure::JsFunctionInfo::of(ns_transform3 as crate::codegen_abi::JsBody3<ClosureHeader>)
        .with_declared(3)
        .with_flags(crate::closure::FN_BUILTIN);
static NS_TRANSFORM_WRITE_INFO: crate::closure::JsFunctionInfo =
    crate::closure::JsFunctionInfo::of(
        ns_transform_write3 as crate::codegen_abi::JsBody3<ClosureHeader>,
    )
    .with_declared(3)
    .with_flags(crate::closure::FN_BUILTIN);
static NS_TRANSFORM_READ_INFO: crate::closure::JsFunctionInfo = crate::closure::JsFunctionInfo::of(
    ns_transform_read0 as crate::codegen_abi::JsBody0<ClosureHeader>,
)
.with_declared(0)
.with_flags(crate::closure::FN_BUILTIN);
static NS_PASSTHROUGH_TRANSFORM_INFO: crate::closure::JsFunctionInfo =
    crate::closure::JsFunctionInfo::of(
        ns_passthrough_transform3 as crate::codegen_abi::JsBody3<ClosureHeader>,
    )
    .with_declared(3)
    .with_flags(crate::closure::FN_BUILTIN);
static NS_TRANSFORM_FINAL_INFO: crate::closure::JsFunctionInfo =
    crate::closure::JsFunctionInfo::of(
        ns_transform_final1 as crate::codegen_abi::JsBody1<ClosureHeader>,
    )
    .with_declared(1)
    .with_flags(crate::closure::FN_BUILTIN);

/// Call `callback(arg)` with an undefined receiver, when it is callable.
fn call_callback(callback: f64, args: &[f64]) {
    if !is_callable_value(callback) {
        return;
    }
    unsafe {
        crate::closure::js_native_call_value(
            callback,
            crate::closure::plain_call_receiver(),
            args.as_ptr(),
            args.len(),
        );
    }
}

/// `Readable.prototype._read(n)`: node's base throws.
extern "C" fn ns_base_read1(
    _c: *const ClosureHeader,
    _this: crate::closure::JsThis,
    _n: f64,
) -> f64 {
    throw_missing_stream_method("The _read() method is not implemented")
}

/// `Writable.prototype._write(chunk, encoding, cb)`: node hands the write to
/// `this._writev([{ chunk, encoding }], cb)` when the stream has one, and
/// throws otherwise.
extern "C" fn ns_base_write3(
    _c: *const ClosureHeader,
    this: crate::closure::JsThis,
    chunk: f64,
    encoding: f64,
    callback: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(this.as_f64());
    let chunk = scope.root_nanbox_f64(chunk);
    let encoding = scope.root_nanbox_f64(encoding);
    let callback = scope.root_nanbox_f64(callback);
    let writev = crate::runtime_state_key!(b"_writev").read_value(stream.get_nanbox_f64());
    if !is_callable_value(writev) {
        throw_missing_stream_method("The _write() method is not implemented")
    }
    let writev = scope.root_nanbox_f64(writev);
    let record = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 2));
    record.with_mut_ptr::<ObjectHeader, _>(|r| {
        js_object_set_field_by_name(r, hidden_key(b"chunk"), chunk.get_nanbox_f64())
    });
    record.with_mut_ptr::<ObjectHeader, _>(|r| {
        js_object_set_field_by_name(r, hidden_key(b"encoding"), encoding.get_nanbox_f64())
    });
    let list = crate::array::js_array_push_f64(
        crate::array::js_array_alloc(0),
        box_pointer(record.get_raw_mut_ptr::<ObjectHeader>() as *const u8),
    );
    let args = [box_pointer(list as *const u8), callback.get_nanbox_f64()];
    unsafe {
        crate::closure::native_call_value_this(
            writev.get_nanbox_f64(),
            crate::closure::JsThis::from_f64(stream.get_nanbox_f64()),
            args.as_ptr(),
            args.len(),
        );
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `Readable/Writable.prototype._destroy(err, cb)`: node's base calls
/// `cb(err)`.
extern "C" fn ns_base_destroy2(
    _c: *const ClosureHeader,
    _this: crate::closure::JsThis,
    err: f64,
    callback: f64,
) -> f64 {
    call_callback(callback, &[err]);
    f64::from_bits(TAG_UNDEFINED)
}

/// `_undestroy()`: node's `destroyImpl.undestroy` — the stream is live again.
extern "C" fn ns_undestroy0(_c: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
    let stream = this.as_f64();
    for slot in [Slot::Destroyed, Slot::Closed, STREAM_CLOSE_EMITTED_KEY] {
        set_hidden_value(stream, slot, f64::from_bits(TAG_FALSE));
    }
    set_hidden_value(stream, hidden_error_key(), f64::from_bits(TAG_UNDEFINED));
    refresh_readable_aborted_flag(stream);
    f64::from_bits(TAG_UNDEFINED)
}

/// `Writable.prototype.pipe()`: node's `errorOrDestroy(this,
/// new ERR_STREAM_CANNOT_PIPE())`.
extern "C" fn ns_cannot_pipe0(_c: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(this.as_f64());
    let msg = b"Cannot pipe, not readable";
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    crate::node_submodules::register_error_code_pub(s, "ERR_STREAM_CANNOT_PIPE");
    let err = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::error::js_error_new_with_message(s) as i64,
    ));
    if stream_auto_destroy_enabled(stream.get_nanbox_f64()) {
        destroy_stream(stream.get_nanbox_f64(), err.get_nanbox_f64());
    } else {
        let error = literal_string_value(b"error");
        let _ = event_emitter::emit_stream_event(
            stream.get_nanbox_f64(),
            error,
            &[err.get_nanbox_f64()],
        );
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `[Symbol.for('nodejs.rejection')](err)`: node's `this.destroy(err)`.
extern "C" fn ns_rejection_destroy1(
    _c: *const ClosureHeader,
    this: crate::closure::JsThis,
    err: f64,
) -> f64 {
    unsafe { crate::runtime_state_key!(b"destroy").call_value(this.as_f64(), &err, 1) };
    f64::from_bits(TAG_UNDEFINED)
}

/// `Readable.prototype[Symbol.for('Stream.toAsyncStreamable')]()`: node's
/// stream/iter interop, which requires `--experimental-stream-iter`.
extern "C" fn ns_to_async_streamable0(
    _c: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    crate::fs::validate::throw_type_error_with_code(
        "The stream/iter API requires the --experimental-stream-iter flag",
        "ERR_STREAM_ITER_MISSING_FLAG",
    )
}

/// Transform's ordinary prototype hook; subclass overrides shadow this
/// method, and super._transform reaches its specified base behavior.
extern "C" fn ns_transform3(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    _chunk: f64,
    _encoding: f64,
    _callback: f64,
) -> f64 {
    throw_missing_stream_method("The _transform() method is not implemented")
}

/// `PassThrough.prototype._transform`: node's `cb(null, chunk)`.
extern "C" fn ns_passthrough_transform3(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    chunk: f64,
    _encoding: f64,
    callback: f64,
) -> f64 {
    call_callback(callback, &[f64::from_bits(TAG_NULL), chunk]);
    f64::from_bits(TAG_UNDEFINED)
}

/// The `(err, data)` callback node's `Transform.prototype._write` hands
/// `_transform`: push `data`, then complete the write.
extern "C" fn ns_transform_write_done(
    c: *const ClosureHeader,
    _this: crate::closure::JsThis,
    err: f64,
    data: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(js_closure_get_capture_f64(c, 0));
    let callback = scope.root_nanbox_f64(js_closure_get_capture_f64(c, 1));
    let err_js = JSValue::from_bits(err.to_bits());
    if !err_js.is_undefined() && !err_js.is_null() {
        call_callback(callback.get_nanbox_f64(), &[err]);
        return f64::from_bits(TAG_UNDEFINED);
    }
    let data_js = JSValue::from_bits(data.to_bits());
    if !data_js.is_undefined() && !data_js.is_null() {
        unsafe { crate::runtime_state_key!(b"push").call_value(stream.get_nanbox_f64(), &data, 1) };
    }
    call_callback(callback.get_nanbox_f64(), &[]);
    f64::from_bits(TAG_UNDEFINED)
}

/// `Transform.prototype._write(chunk, encoding, callback)`: node runs
/// `this._transform(chunk, encoding, (err, data) => ...)`.
extern "C" fn ns_transform_write3(
    _c: *const ClosureHeader,
    this: crate::closure::JsThis,
    chunk: f64,
    encoding: f64,
    callback: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(this.as_f64());
    let chunk = scope.root_nanbox_f64(chunk);
    let encoding = scope.root_nanbox_f64(encoding);
    let callback = scope.root_nanbox_f64(callback);
    let transform = scope.root_nanbox_f64(
        crate::runtime_state_key!(b"_transform").read_value(stream.get_nanbox_f64()),
    );
    let done = scope.root_raw_mut_ptr(js_closure_alloc(
        crate::fn_info!(ns_transform_write_done, 2; with_declared(2)),
        2,
    ));
    done.with_mut_ptr::<ClosureHeader, _>(|d| {
        js_closure_set_capture_f64(d, 0, stream.get_nanbox_f64());
        js_closure_set_capture_f64(d, 1, callback.get_nanbox_f64());
    });
    let args = [
        chunk.get_nanbox_f64(),
        encoding.get_nanbox_f64(),
        box_pointer(done.get_raw_mut_ptr::<ClosureHeader>() as *const u8),
    ];
    unsafe {
        crate::closure::native_call_value_this(
            transform.get_nanbox_f64(),
            crate::closure::JsThis::from_f64(stream.get_nanbox_f64()),
            args.as_ptr(),
            args.len(),
        );
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `Transform.prototype._read()`: node resumes a write held back by
/// backpressure; this `_write` never holds one.
extern "C" fn ns_transform_read0(_c: *const ClosureHeader, _this: crate::closure::JsThis) -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

/// The callback node's `Transform.prototype._final` hands `_flush`.
extern "C" fn ns_transform_final_done(
    c: *const ClosureHeader,
    _this: crate::closure::JsThis,
    err: f64,
    data: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(js_closure_get_capture_f64(c, 0));
    let callback = scope.root_nanbox_f64(js_closure_get_capture_f64(c, 1));
    let err_js = JSValue::from_bits(err.to_bits());
    if !err_js.is_undefined() && !err_js.is_null() {
        if is_callable_value(callback.get_nanbox_f64()) {
            call_callback(callback.get_nanbox_f64(), &[err]);
        } else {
            destroy_stream(stream.get_nanbox_f64(), err);
        }
        return f64::from_bits(TAG_UNDEFINED);
    }
    let data_js = JSValue::from_bits(data.to_bits());
    if !data_js.is_undefined() && !data_js.is_null() {
        unsafe { crate::runtime_state_key!(b"push").call_value(stream.get_nanbox_f64(), &data, 1) };
    }
    let eof = f64::from_bits(TAG_NULL);
    unsafe { crate::runtime_state_key!(b"push").call_value(stream.get_nanbox_f64(), &eof, 1) };
    call_callback(callback.get_nanbox_f64(), &[]);
    f64::from_bits(TAG_UNDEFINED)
}

/// `Transform.prototype._final(cb)`: node runs `this._flush` when the stream
/// has one, then ends the readable side.
extern "C" fn ns_transform_final1(
    _c: *const ClosureHeader,
    this: crate::closure::JsThis,
    callback: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(this.as_f64());
    let callback = scope.root_nanbox_f64(callback);
    let flush = scope
        .root_nanbox_f64(crate::runtime_state_key!(b"_flush").read_value(stream.get_nanbox_f64()));
    let done = scope.root_raw_mut_ptr(js_closure_alloc(
        crate::fn_info!(ns_transform_final_done, 2; with_declared(2)),
        2,
    ));
    done.with_mut_ptr::<ClosureHeader, _>(|d| {
        js_closure_set_capture_f64(d, 0, stream.get_nanbox_f64());
        js_closure_set_capture_f64(d, 1, callback.get_nanbox_f64());
    });
    let done_value = box_pointer(done.get_raw_mut_ptr::<ClosureHeader>() as *const u8);
    if is_callable_value(flush.get_nanbox_f64()) && !stream_destroyed(stream.get_nanbox_f64()) {
        unsafe {
            crate::closure::native_call_value_this(
                flush.get_nanbox_f64(),
                crate::closure::JsThis::from_f64(stream.get_nanbox_f64()),
                &done_value,
                1,
            );
        }
    } else {
        let undefined = f64::from_bits(TAG_UNDEFINED);
        ns_transform_final_done(
            done.get_raw_mut_ptr::<ClosureHeader>(),
            crate::closure::JsThis::from_f64(stream.get_nanbox_f64()),
            undefined,
            undefined,
        );
    }
    f64::from_bits(TAG_UNDEFINED)
}

// ─────────────────────────────────────────────────────────────────
// Stream flags are inherited getters, as in Node. Their backing slots are
// runtime-owned record fields, never extra string keys on the instance; the
// public data (`readableLength`, …) read their state object's own property.
// ─────────────────────────────────────────────────────────────────

/// One stream accessor of the prototypes' layouts.
#[derive(Clone, Copy)]
#[repr(u8)]
enum Prop {
    Destroyed,
    Closed,
    Errored,
    ReadableAborted,
    ReadableLength,
    ReadableHwm,
    ReadableObjectMode,
    ReadableFlowing,
    Readable,
    ReadableEnded,
    ReadableDidRead,
    ReadableEncoding,
    ReadableBuffer,
    WritableObjectMode,
    WritableHwm,
    WritableLength,
    WritableNeedDrain,
    WritableCorked,
    Writable,
    WritableEnded,
    WritableFinished,
    WritableBuffer,
    WritableAborted,
}

const PROPS: [Prop; 23] = [
    Prop::Destroyed,
    Prop::Closed,
    Prop::Errored,
    Prop::ReadableAborted,
    Prop::ReadableLength,
    Prop::ReadableHwm,
    Prop::ReadableObjectMode,
    Prop::ReadableFlowing,
    Prop::Readable,
    Prop::ReadableEnded,
    Prop::ReadableDidRead,
    Prop::ReadableEncoding,
    Prop::ReadableBuffer,
    Prop::WritableObjectMode,
    Prop::WritableHwm,
    Prop::WritableLength,
    Prop::WritableNeedDrain,
    Prop::WritableCorked,
    Prop::Writable,
    Prop::WritableEnded,
    Prop::WritableFinished,
    Prop::WritableBuffer,
    Prop::WritableAborted,
];

impl Prop {
    /// The record flag a setter writes (node's setters on these four).
    fn settable(self) -> Option<Slot> {
        match self {
            Prop::Destroyed => Some(Slot::Destroyed),
            Prop::Readable => Some(Slot::Readable),
            Prop::ReadableFlowing => Some(Slot::ReadableFlowing),
            Prop::Writable => Some(Slot::Writable),
            _ => None,
        }
    }

    fn get(self, stream: f64) -> f64 {
        let undefined = f64::from_bits(TAG_UNDEFINED);
        let slot = |slot: Slot| read_slot(stream, slot).unwrap_or(undefined);
        let data = |data: StateData| read_state_data(stream, data).unwrap_or(undefined);
        let flag = |b: bool| f64::from_bits(if b { TAG_TRUE } else { TAG_FALSE });
        match self {
            Prop::Destroyed => slot(Slot::Destroyed),
            Prop::Closed => slot(Slot::Closed),
            Prop::Errored => readable_hidden_error(stream).unwrap_or(f64::from_bits(TAG_NULL)),
            Prop::ReadableAborted => slot(Slot::ReadableAborted),
            Prop::ReadableLength => data(StateData::ReadableBuffered),
            Prop::ReadableHwm => data(StateData::ReadableHwm),
            Prop::ReadableObjectMode => slot(Slot::ReadableObjectMode),
            Prop::ReadableFlowing => slot(Slot::ReadableFlowing),
            Prop::Readable => slot(Slot::Readable),
            Prop::ReadableEnded => slot(Slot::EndEmitted),
            Prop::ReadableDidRead => slot(Slot::Disturbed),
            Prop::ReadableEncoding => slot(Slot::ReadableEncoding),
            Prop::ReadableBuffer => match get_hidden_value(stream, hidden_chunks_key()) {
                Some(value) if is_array_like_value(value) => value,
                _ => box_pointer(crate::array::js_array_alloc(0) as *const u8),
            },
            Prop::WritableObjectMode => slot(Slot::WritableObjectMode),
            Prop::WritableHwm => data(StateData::WritableHwm),
            Prop::WritableLength => data(StateData::WritableLength),
            Prop::WritableNeedDrain => slot(Slot::WritableNeedDrain),
            Prop::WritableCorked => data(StateData::WritableCorked),
            Prop::Writable => slot(Slot::Writable),
            Prop::WritableEnded => slot(Slot::WritableEnded),
            Prop::WritableFinished => slot(Slot::WritableFinished),
            Prop::WritableBuffer => match buffered_writable_writes(stream) {
                Some(value) if is_array_like_value(value) => {
                    let arr = raw_ptr_from_value(value) as *const crate::array::ArrayHeader;
                    build_writev_chunks(arr, crate::array::js_array_length(arr))
                }
                _ => box_pointer(crate::array::js_array_alloc(0) as *const u8),
            },
            // node: `writable !== false && (destroyed || errored) && !finished`.
            Prop::WritableAborted => {
                let writable_option = read_slot(stream, Slot::WritableOption)
                    .is_none_or(|v| v.to_bits() != TAG_FALSE);
                flag(
                    writable_option
                        && (stream_destroyed(stream) || readable_hidden_error(stream).is_some())
                        && !has_truthy_hidden(stream, hidden_finish_emitted_key()),
                )
            }
        }
    }
}

extern "C" fn stream_property_get(c: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
    let index = js_closure_get_capture_f64(c, 0) as usize;
    match PROPS.get(index) {
        Some(prop) => prop.get(this.as_f64()),
        None => f64::from_bits(TAG_UNDEFINED),
    }
}
extern "C" fn stream_property_set(
    c: *const ClosureHeader,
    this: crate::closure::JsThis,
    v: f64,
) -> f64 {
    let index = js_closure_get_capture_f64(c, 0) as usize;
    if let Some(slot) = PROPS.get(index).and_then(|prop| prop.settable()) {
        write_slot(this.as_f64(), slot, v);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// Fill the accessor `name` of `layout` with `prop`'s getter (and setter).
fn install_prop(layout: &layouts::NodePrototype, proto: *mut ObjectHeader, name: &str, prop: Prop) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto = scope.root_raw_mut_ptr(proto);
    let get = scope.root_raw_mut_ptr(js_closure_alloc(crate::fn_info!(stream_property_get, 0), 1));
    get.with_mut_ptr::<ClosureHeader, _>(|c| js_closure_set_capture_f64(c, 0, prop as u8 as f64));
    let set_bits = if prop.settable().is_some() {
        let set =
            scope.root_raw_mut_ptr(js_closure_alloc(crate::fn_info!(stream_property_set, 1), 1));
        set.with_mut_ptr::<ClosureHeader, _>(|c| {
            js_closure_set_capture_f64(c, 0, prop as u8 as f64)
        });
        box_pointer(set.get_raw_mut_ptr::<ClosureHeader>().cast()).to_bits()
    } else {
        0
    };
    let get_bits = box_pointer(get.get_raw_mut_ptr::<ClosureHeader>().cast()).to_bits();
    // SAFETY: the fresh prototype born on `layout`; the key is its accessor.
    unsafe {
        layout.accessor(
            proto.get_raw_mut_ptr::<ObjectHeader>(),
            name.as_bytes(),
            get_bits,
            set_bits,
        )
    };
}

// ─────────────────────────────────────────────────────────────────
// The legacy `Stream.prototype` (node's `lib/internal/streams/legacy.js`):
// `constructor`, `pipe` and `eventNames`, over `EventEmitter.prototype`.
// ─────────────────────────────────────────────────────────────────

/// Build `Stream.prototype` for `constructor`: born on its layout (whose
/// [[Prototype]] is `EventEmitter.prototype`), filled, sealed.
pub(crate) fn build_legacy_stream_prototype(constructor: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let constructor = scope.root_nanbox_f64(constructor);
    // SAFETY: a node prototype descriptor of this runtime.
    let layout =
        unsafe { layouts::NodePrototype::of(std::ptr::addr_of!(layouts::STREAM_PROTOTYPE)) };
    let proto = scope.root_raw_mut_ptr(unsafe { layout.birth() });
    let p = || proto.get_raw_mut_ptr::<ObjectHeader>();
    let pipe = proto_closure(&scope, &NS_LEGACY_PIPE_INFO);
    let event_names = proto_closure(
        &scope,
        emitter_methods()
            .iter()
            .find(|(name, _)| *name == "eventNames")
            .map(|(_, info)| *info)
            .expect("eventNames is an emitter method"),
    );
    // SAFETY: the fresh prototype born on `layout`, not yet reachable.
    unsafe {
        layout.data(p(), b"constructor", constructor.get_nanbox_f64());
        layout.data(p(), b"pipe", pipe.get_nanbox_f64());
        layout.data(p(), b"eventNames", event_names.get_nanbox_f64());
        layout.seal(
            p(),
            crate::closure::shape::INTRINSIC_SERIAL_STREAM_PROTOTYPE,
        );
    }
    proto.with_mut_ptr::<ObjectHeader, _>(|proto| crate::value::js_nanbox_pointer(proto as i64))
}

static NS_LEGACY_PIPE_INFO: crate::closure::JsFunctionInfo = crate::closure::JsFunctionInfo::of(
    ns_legacy_pipe2 as crate::codegen_abi::JsBody2<ClosureHeader>,
)
.with_declared(2)
.with_flags(crate::closure::FN_BUILTIN);

/// The legacy pipe's shared record: `[source, dest, ondata, ondrain, onend,
/// onclose, onerror, cleanup, didOnEnd]`.
mod pipe_slot {
    pub(super) const SOURCE: u32 = 0;
    pub(super) const DEST: u32 = 1;
    pub(super) const ONDATA: u32 = 2;
    pub(super) const ONDRAIN: u32 = 3;
    pub(super) const ONEND: u32 = 4;
    pub(super) const ONCLOSE: u32 = 5;
    pub(super) const ONERROR: u32 = 6;
    pub(super) const CLEANUP: u32 = 7;
    pub(super) const DID_ON_END: u32 = 8;
    pub(super) const LEN: u32 = 9;
}

fn pipe_get(record: f64, slot: u32) -> f64 {
    crate::array::js_array_get_f64(
        raw_ptr_from_value(record) as *const crate::array::ArrayHeader,
        slot,
    )
}
fn pipe_set(record: f64, slot: u32, value: f64) {
    crate::array::js_array_set_f64(
        raw_ptr_from_value(record) as *mut crate::array::ArrayHeader,
        slot,
        value,
    );
}
fn truthy(value: f64) -> bool {
    crate::value::js_is_truthy(value) != 0
}
/// `target[name](...args)`.
fn call_named(target: f64, name: crate::runtime_state_key::NamedStateKey, args: &[f64]) -> f64 {
    unsafe { name.call_value(target, args.as_ptr(), args.len()) }
}

extern "C" fn legacy_pipe_ondata(
    c: *const ClosureHeader,
    _t: crate::closure::JsThis,
    chunk: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let record = scope.root_nanbox_f64(js_closure_get_capture_f64(c, 0));
    let dest = scope.root_nanbox_f64(pipe_get(record.get_nanbox_f64(), pipe_slot::DEST));
    if truthy(crate::runtime_state_key!(b"writable").read_value(dest.get_nanbox_f64())) {
        let wrote = call_named(
            dest.get_nanbox_f64(),
            crate::runtime_state_key!(b"write"),
            &[chunk],
        );
        if wrote.to_bits() == TAG_FALSE {
            let source =
                scope.root_nanbox_f64(pipe_get(record.get_nanbox_f64(), pipe_slot::SOURCE));
            if truthy(crate::runtime_state_key!(b"pause").read_value(source.get_nanbox_f64())) {
                call_named(
                    source.get_nanbox_f64(),
                    crate::runtime_state_key!(b"pause"),
                    &[],
                );
            }
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn legacy_pipe_ondrain(c: *const ClosureHeader, _t: crate::closure::JsThis) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let record = scope.root_nanbox_f64(js_closure_get_capture_f64(c, 0));
    let source = scope.root_nanbox_f64(pipe_get(record.get_nanbox_f64(), pipe_slot::SOURCE));
    if truthy(crate::runtime_state_key!(b"readable").read_value(source.get_nanbox_f64()))
        && truthy(crate::runtime_state_key!(b"resume").read_value(source.get_nanbox_f64()))
    {
        call_named(
            source.get_nanbox_f64(),
            crate::runtime_state_key!(b"resume"),
            &[],
        );
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `onend` (`end`: true) and `onclose` (false).
fn legacy_pipe_finish(c: *const ClosureHeader, end: bool) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let record = scope.root_nanbox_f64(js_closure_get_capture_f64(c, 0));
    if pipe_get(record.get_nanbox_f64(), pipe_slot::DID_ON_END).to_bits() == TAG_TRUE {
        return f64::from_bits(TAG_UNDEFINED);
    }
    pipe_set(
        record.get_nanbox_f64(),
        pipe_slot::DID_ON_END,
        f64::from_bits(TAG_TRUE),
    );
    let dest = scope.root_nanbox_f64(pipe_get(record.get_nanbox_f64(), pipe_slot::DEST));
    if end {
        call_named(
            dest.get_nanbox_f64(),
            crate::runtime_state_key!(b"end"),
            &[],
        );
    } else if is_callable_value(
        crate::runtime_state_key!(b"destroy").read_value(dest.get_nanbox_f64()),
    ) {
        call_named(
            dest.get_nanbox_f64(),
            crate::runtime_state_key!(b"destroy"),
            &[],
        );
    }
    f64::from_bits(TAG_UNDEFINED)
}
extern "C" fn legacy_pipe_onend(c: *const ClosureHeader, _t: crate::closure::JsThis) -> f64 {
    legacy_pipe_finish(c, true)
}
extern "C" fn legacy_pipe_onclose(c: *const ClosureHeader, _t: crate::closure::JsThis) -> f64 {
    legacy_pipe_finish(c, false)
}

/// Remove every listener the pipe added.
extern "C" fn legacy_pipe_cleanup(c: *const ClosureHeader, _t: crate::closure::JsThis) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let record = scope.root_nanbox_f64(js_closure_get_capture_f64(c, 0));
    let r = &record;
    let source = scope.root_nanbox_f64(pipe_get((r.get_nanbox_f64()), pipe_slot::SOURCE));
    let dest = scope.root_nanbox_f64(pipe_get((r.get_nanbox_f64()), pipe_slot::DEST));
    for (target, event, slot) in [
        (&source, &b"data"[..], pipe_slot::ONDATA),
        (&dest, b"drain", pipe_slot::ONDRAIN),
        (&source, b"end", pipe_slot::ONEND),
        (&source, b"close", pipe_slot::ONCLOSE),
        (&source, b"error", pipe_slot::ONERROR),
        (&dest, b"error", pipe_slot::ONERROR),
        (&source, b"end", pipe_slot::CLEANUP),
        (&source, b"close", pipe_slot::CLEANUP),
        (&dest, b"close", pipe_slot::CLEANUP),
    ] {
        let event = scope.root_nanbox_f64(literal_string_value(event_static(event)));
        let listener = pipe_get((r.get_nanbox_f64()), slot);
        call_named(
            target.get_nanbox_f64(),
            crate::runtime_state_key!(b"removeListener"),
            &[event.get_nanbox_f64(), listener],
        );
    }
    f64::from_bits(TAG_UNDEFINED)
}

fn event_static(event: &[u8]) -> &'static [u8] {
    match event {
        b"data" => b"data",
        b"drain" => b"drain",
        b"end" => b"end",
        b"close" => b"close",
        _ => b"error",
    }
}

/// `onerror`: clean up, and rethrow on an emitter nobody else listens to.
extern "C" fn legacy_pipe_onerror(
    c: *const ClosureHeader,
    this: crate::closure::JsThis,
    er: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let record = scope.root_nanbox_f64(js_closure_get_capture_f64(c, 0));
    let emitter = scope.root_nanbox_f64(this.as_f64());
    let er = scope.root_nanbox_f64(er);
    let cleanup = pipe_get(record.get_nanbox_f64(), pipe_slot::CLEANUP);
    call_callback(cleanup, &[]);
    let error = scope.root_nanbox_f64(literal_string_value(b"error"));
    if event_emitter::stream_listener_count_for_event(
        emitter.get_nanbox_f64(),
        error.get_nanbox_f64(),
    ) == 0
    {
        call_named(
            emitter.get_nanbox_f64(),
            crate::runtime_state_key!(b"emit"),
            &[error.get_nanbox_f64(), er.get_nanbox_f64()],
        );
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `Stream.prototype.pipe(dest, options)`: node's legacy pipe.
extern "C" fn ns_legacy_pipe2(
    _c: *const ClosureHeader,
    this: crate::closure::JsThis,
    dest: f64,
    options: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let source = scope.root_nanbox_f64(this.as_f64());
    let dest = scope.root_nanbox_f64(dest);
    let options = scope.root_nanbox_f64(options);
    let record = scope.root_nanbox_f64(box_pointer(
        crate::array::js_array_alloc_with_length_exact(pipe_slot::LEN) as *const u8,
    ));
    let r = &record;
    pipe_set(
        (r.get_nanbox_f64()),
        pipe_slot::SOURCE,
        source.get_nanbox_f64(),
    );
    pipe_set((r.get_nanbox_f64()), pipe_slot::DEST, dest.get_nanbox_f64());
    pipe_set(
        (r.get_nanbox_f64()),
        pipe_slot::DID_ON_END,
        f64::from_bits(TAG_FALSE),
    );
    for (slot, info) in [
        (
            pipe_slot::ONDATA,
            crate::fn_info!(legacy_pipe_ondata, 1; with_declared(1)),
        ),
        (
            pipe_slot::ONDRAIN,
            crate::fn_info!(legacy_pipe_ondrain, 0; with_declared(0)),
        ),
        (
            pipe_slot::ONEND,
            crate::fn_info!(legacy_pipe_onend, 0; with_declared(0)),
        ),
        (
            pipe_slot::ONCLOSE,
            crate::fn_info!(legacy_pipe_onclose, 0; with_declared(0)),
        ),
        (
            pipe_slot::ONERROR,
            crate::fn_info!(legacy_pipe_onerror, 1; with_declared(1)),
        ),
        (
            pipe_slot::CLEANUP,
            crate::fn_info!(legacy_pipe_cleanup, 0; with_declared(0)),
        ),
    ] {
        let closure = js_closure_alloc(info, 1);
        js_closure_set_capture_f64(closure, 0, (r.get_nanbox_f64()));
        pipe_set(
            (r.get_nanbox_f64()),
            slot,
            box_pointer(closure as *const u8),
        );
    }
    let on = |target: f64, event: &'static [u8], slot: u32| {
        let event = scope.root_nanbox_f64(literal_string_value(event));
        let listener = pipe_get((r.get_nanbox_f64()), slot);
        call_named(
            target,
            crate::runtime_state_key!(b"on"),
            &[event.get_nanbox_f64(), listener],
        );
    };
    on(source.get_nanbox_f64(), b"data", pipe_slot::ONDATA);
    on(dest.get_nanbox_f64(), b"drain", pipe_slot::ONDRAIN);
    let is_stdio = truthy(crate::runtime_state_key!(b"_isStdio").read_value(dest.get_nanbox_f64()));
    let end_option = JSValue::from_bits(options.get_nanbox_u64()).is_pointer()
        && crate::runtime_state_key!(b"end")
            .read_value(options.get_nanbox_f64())
            .to_bits()
            == TAG_FALSE;
    if !is_stdio && !end_option {
        on(source.get_nanbox_f64(), b"end", pipe_slot::ONEND);
        on(source.get_nanbox_f64(), b"close", pipe_slot::ONCLOSE);
    }
    for target in [source.get_nanbox_f64(), dest.get_nanbox_f64()] {
        let event = scope.root_nanbox_f64(literal_string_value(b"error"));
        let listener = pipe_get((r.get_nanbox_f64()), pipe_slot::ONERROR);
        call_named(
            target,
            crate::runtime_state_key!(b"prependListener"),
            &[event.get_nanbox_f64(), listener],
        );
    }
    on(source.get_nanbox_f64(), b"end", pipe_slot::CLEANUP);
    on(source.get_nanbox_f64(), b"close", pipe_slot::CLEANUP);
    on(dest.get_nanbox_f64(), b"close", pipe_slot::CLEANUP);
    let pipe = scope.root_nanbox_f64(literal_string_value(b"pipe"));
    call_named(
        dest.get_nanbox_f64(),
        crate::runtime_state_key!(b"emit"),
        &[pipe.get_nanbox_f64(), source.get_nanbox_f64()],
    );
    dest.get_nanbox_f64()
}

crate::realm_decl!("proto_methods.realm.json#*");
