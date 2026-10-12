//! G1 (STREAM-PAYLOAD-DESIGN): the classic stream methods live on the stream
//! prototypes, as in node, not as bound closures on every instance.
//!
//! `Readable.prototype`, `Writable.prototype` and `Duplex.prototype` each get
//! their method set once per realm, when the constructor's prototype is built
//! (`object::native_module_stream::attach_stream_constructor_prototype`). Each
//! method is one IMPLICIT_THIS closure (capture slot 0 holds `undefined`, so
//! `this_value` reads the call-site receiver) shared by every instance:
//! `Transform`/`PassThrough` and their subclasses inherit them through the
//! prototype chain (`Transform.prototype -> Duplex.prototype ->
//! Readable.prototype -> Stream.prototype -> EventEmitter.prototype`), a
//! source-compiled `class X extends Readable` through its class prototype's
//! parent edge, and a payload family (zlib, crypto) through its family
//! prototype. A subclass override shadows the inherited method by ordinary
//! lookup, and `super.m()` reaches the base body on the chain.
//!
//! Instances own Node's enumerable state and optional option-supplied hooks.
//! The private record adds no string keys, so reflection and JSON use the
//! same ordinary shape and own-property order.

use super::*;
use crate::object::ObjectHeader;

/// Which method set a stream prototype carries.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum StreamProto {
    Readable,
    Writable,
    Duplex,
    Transform,
}

/// Install `kind`'s method set (and its symbol-keyed methods) on `proto`.
pub(crate) fn install_stream_prototype_methods(proto: *mut ObjectHeader, kind: StreamProto) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto = scope.root_raw_mut_ptr(proto);
    let readable = readable_methods();
    let writable = writable_methods();
    let methods: Vec<(&'static str, StubFn)> = match kind {
        StreamProto::Readable => readable.to_vec(),
        // The base hook is a runtime builtin: the constructor's subclass-hook
        // capture (`user_hook`) skips builtins, so an inherited base body is
        // never mistaken for a subclass override. Without the flag
        // `class P extends PassThrough {}` captured this throwing body as its
        // transform and lost PassThrough's identity transform.
        StreamProto::Transform => vec![(
            "_transform",
            crate::fn_info!(ns_transform3, 3; with_declared(3), with_flags(crate::closure::FN_BUILTIN)),
        )],
        // `_write` is the user's hook, not a prototype method the runtime
        // calls: a prototype `_write` would read as a subclass override.
        StreamProto::Writable => writable
            .iter()
            .copied()
            .filter(|(name, _)| *name != "_write")
            .collect(),
        // node's Duplex.prototype adds the writable half; the readable half
        // and the emitter methods come from Readable.prototype.
        StreamProto::Duplex => writable
            .iter()
            .copied()
            .filter(|(name, _)| {
                matches!(
                    *name,
                    "write" | "end" | "cork" | "uncork" | "setDefaultEncoding" | "destroy"
                )
            })
            .collect(),
    };
    let mut on_value: Option<crate::gc::RuntimeHandle<'_>> = None;
    for (name, info) in methods {
        // `addListener` IS `on` (node's alias), so both names hold one value.
        let value = match (name, &on_value) {
            ("addListener", Some(on)) => scope.root_nanbox_f64(on.get_nanbox_f64()),
            _ => {
                let closure = crate::closure::js_closure_alloc(info, 1);
                crate::closure::js_closure_set_capture_ptr(
                    closure,
                    0,
                    crate::value::TAG_UNDEFINED as i64,
                );
                scope.root_nanbox_f64(box_pointer(closure as *const u8))
            }
        };
        if name == "on" {
            on_value = Some(scope.root_nanbox_f64(value.get_nanbox_f64()));
        }
        let key = scope.root_string_ptr(hidden_key(name.as_bytes()));
        proto.with_mut_ptr::<ObjectHeader, _>(|proto| {
            key.with_const_ptr::<crate::StringHeader, _>(|key| {
                crate::object::js_object_set_field_by_name(proto, key, value.get_nanbox_f64())
            })
        });
    }
    install_stream_property_accessors(proto.get_raw_mut_ptr::<ObjectHeader>(), kind);
    let proto_value = proto
        .with_mut_ptr::<ObjectHeader, _>(|proto| crate::value::js_nanbox_pointer(proto as i64));
    let proto_value = scope.root_nanbox_f64(proto_value);
    if kind == StreamProto::Readable {
        async_iterator::install_readable_async_iterator_symbol_on_prototype(
            proto_value.get_nanbox_f64(),
        );
    }
    if matches!(kind, StreamProto::Readable | StreamProto::Writable) {
        install_async_dispose_symbol_on_prototype(proto_value.get_nanbox_f64());
    }
}

/// `Readable.prototype[Symbol.asyncDispose]` / `Writable.prototype[...]`:
/// one receiver-from-`this` closure.
fn install_async_dispose_symbol_on_prototype(proto: f64) {
    let async_dispose = crate::symbol::well_known_symbol("asyncDispose");
    if async_dispose.is_null() {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto = scope.root_nanbox_f64(proto);
    let closure = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(ns_async_dispose, 0; with_declared(0)),
        1,
    ));
    closure.with_mut_ptr::<ClosureHeader, _>(|closure| {
        crate::closure::js_closure_set_capture_ptr(closure, 0, crate::value::TAG_UNDEFINED as i64)
    });
    let closure_value =
        closure.with_mut_ptr::<ClosureHeader, _>(|closure| box_pointer(closure as *const u8));
    unsafe {
        crate::symbol::js_object_set_symbol_property(
            proto.get_nanbox_f64(),
            box_pointer(async_dispose as *const u8),
            closure_value,
        );
    }
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
    opts: f64,
    kind: super::constructors::birth::Kind,
) -> (f64, Option<super::constructors::birth_memo::Input>) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let opts = scope.root_nanbox_f64(opts);
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
    let (stream, proof) =
        super::constructors::birth_memo::alloc(proto.get_nanbox_f64(), opts.get_nanbox_f64(), kind);
    if let Some(stream) = stream {
        return (stream, proof);
    }
    (
        super::constructors::alloc_initialized_stream_shell(proto.get_nanbox_f64(), kind),
        proof,
    )
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

// Stream flags are inherited getters, as in Node. Their backing slots are
// runtime-owned record fields, never extra string keys on the instance; the
// public data (`readableLength`, …) read their state object's own property.
#[derive(Clone, Copy)]
enum Word {
    Slot(Slot),
    Data(StateData),
}
const READABLE_PROPERTIES: &[(&str, Word)] = &[
    ("destroyed", Word::Slot(Slot::Destroyed)),
    ("closed", Word::Slot(Slot::Closed)),
    ("readableAborted", Word::Slot(Slot::ReadableAborted)),
    ("readableLength", Word::Data(StateData::ReadableBuffered)),
    ("readableHighWaterMark", Word::Data(StateData::ReadableHwm)),
    ("readableObjectMode", Word::Slot(Slot::ReadableObjectMode)),
    ("readableFlowing", Word::Slot(Slot::ReadableFlowing)),
    ("readable", Word::Slot(Slot::Readable)),
    ("readableEnded", Word::Slot(Slot::ReadableEnded)),
    ("readableDidRead", Word::Slot(Slot::Disturbed)),
    ("readableEncoding", Word::Slot(Slot::ReadableEncoding)),
];
const WRITABLE_PROPERTIES: &[(&str, Word)] = &[
    ("destroyed", Word::Slot(Slot::Destroyed)),
    ("closed", Word::Slot(Slot::Closed)),
    ("writableObjectMode", Word::Slot(Slot::WritableObjectMode)),
    ("writableHighWaterMark", Word::Data(StateData::WritableHwm)),
    ("writableLength", Word::Data(StateData::WritableLength)),
    ("writableNeedDrain", Word::Slot(Slot::WritableNeedDrain)),
    ("writableCorked", Word::Data(StateData::WritableCorked)),
    ("writable", Word::Slot(Slot::Writable)),
    ("writableEnded", Word::Slot(Slot::WritableEnded)),
    ("writableFinished", Word::Slot(Slot::WritableFinished)),
];

extern "C" fn stream_property_get(c: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
    let index = js_closure_get_capture_f64(c, 0) as usize;
    let properties = if index < 64 {
        READABLE_PROPERTIES
    } else {
        WRITABLE_PROPERTIES
    };
    let Some((_, slot)) = properties.get(index % 64) else {
        return f64::from_bits(TAG_UNDEFINED);
    };
    match *slot {
        Word::Data(data) => read_state_data(this.as_f64(), data),
        Word::Slot(slot) => read_slot(this.as_f64(), slot),
    }
    .unwrap_or(f64::from_bits(TAG_UNDEFINED))
}
extern "C" fn stream_property_set(
    c: *const ClosureHeader,
    this: crate::closure::JsThis,
    v: f64,
) -> f64 {
    let index = js_closure_get_capture_f64(c, 0) as usize;
    let properties = if index < 64 {
        READABLE_PROPERTIES
    } else {
        WRITABLE_PROPERTIES
    };
    // Only record flags get a setter (see install_stream_property_accessors).
    if let Some(&(_, Word::Slot(slot))) = properties.get(index % 64) {
        write_slot(this.as_f64(), slot, v);
    }
    f64::from_bits(TAG_UNDEFINED)
}
fn install_stream_property_accessors(proto: *mut ObjectHeader, kind: StreamProto) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto = scope.root_raw_mut_ptr(proto);
    let sets: &[(&[(&str, Word)], usize)] = match kind {
        StreamProto::Readable => &[(READABLE_PROPERTIES, 0)],
        StreamProto::Writable => &[(WRITABLE_PROPERTIES, 64)],
        StreamProto::Duplex => &[(WRITABLE_PROPERTIES, 64)],
        StreamProto::Transform => &[],
    };
    for &(properties, base) in sets {
        for (i, &(name, _)) in properties.iter().enumerate() {
            let get = scope
                .root_raw_mut_ptr(js_closure_alloc(crate::fn_info!(stream_property_get, 0), 1));
            get.with_mut_ptr::<ClosureHeader, _>(|c| {
                js_closure_set_capture_f64(c, 0, (base + i) as f64)
            });
            let set_bits = if matches!(
                properties[i].1,
                Word::Slot(
                    Slot::Destroyed | Slot::Readable | Slot::ReadableFlowing | Slot::Writable
                )
            ) {
                let set = scope
                    .root_raw_mut_ptr(js_closure_alloc(crate::fn_info!(stream_property_set, 1), 1));
                set.with_mut_ptr::<ClosureHeader, _>(|c| {
                    js_closure_set_capture_f64(c, 0, (base + i) as f64)
                });
                box_pointer(set.get_raw_mut_ptr::<ClosureHeader>().cast()).to_bits()
            } else {
                TAG_UNDEFINED
            };
            let key = scope
                .root_string_ptr(crate::string::intern_ascii_literal(name.as_bytes()) as *mut _);
            unsafe {
                crate::object::ensure_key_in_keys_array(
                    proto.get_raw_mut_ptr::<ObjectHeader>(),
                    key.get_raw_const_ptr::<crate::StringHeader>(),
                );
            }
            crate::object::install_fresh_accessor_property(
                proto.get_raw_mut_ptr::<ObjectHeader>() as usize,
                name.to_owned(),
                crate::object::AccessorDescriptor {
                    get: box_pointer(get.get_raw_mut_ptr::<ClosureHeader>().cast()).to_bits(),
                    set: set_bits,
                },
                crate::object::PropertyAttrs::new(true, false, true),
            );
        }
    }
}
