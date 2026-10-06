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
//! Instances therefore own only their state, which the runtime defines
//! non-enumerable (`set_hidden_value`), so `Object.keys`, `JSON.stringify`
//! and `for…in` skip it structurally, with no key filter.

use super::*;
use crate::object::ObjectHeader;

/// Which method set a stream prototype carries.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum StreamProto {
    Readable,
    Writable,
    Duplex,
}

/// Install `kind`'s method set (and its symbol-keyed methods) on `proto`.
pub(crate) fn install_stream_prototype_methods(proto: *mut ObjectHeader, kind: StreamProto) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto = scope.root_raw_mut_ptr(proto);
    let readable = readable_methods();
    let writable = writable_methods();
    let methods: Vec<(&'static str, StubFn)> = match kind {
        StreamProto::Readable => readable.to_vec(),
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
    let proto_value = proto
        .with_mut_ptr::<ObjectHeader, _>(|proto| crate::value::js_nanbox_pointer(proto as i64));
    let proto_value = scope.root_nanbox_f64(proto_value);
    if kind == StreamProto::Readable {
        async_iterator::install_readable_async_iterator_symbol_on_prototype(
            proto_value.get_nanbox_f64(),
        );
    }
    if kind != StreamProto::Duplex {
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
pub(super) fn alloc_stream_instance(name: &str) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto = scope.root_nanbox_f64(stream_prototype_value(name));
    if !JSValue::from_bits(proto.get_nanbox_u64()).is_pointer() {
        return box_pointer(crate::object::js_object_alloc(0, 0) as *const u8);
    }
    crate::object::js_object_create(proto.get_nanbox_f64())
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
