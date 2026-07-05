//! Shared `[[Construct]]`-only checks and `new.target`-aware prototype
//! resolution used by the Intl service constructors (#5835). Split out of
//! `intl.rs` to keep that file under the repository's 2,000-line gate.

use crate::closure::ClosureHeader;
use crate::object::{js_object_get_field_by_name_f64, ObjectHeader, PropertyAttrs};
use crate::string::js_string_from_bytes;
use crate::value::JSValue;

const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

/// `GetPrototypeFromConstructor(new.target, "%<Ctor>Prototype%")`: a
/// `Reflect.construct(Intl.X, args, CustomCtor)` call should install
/// `CustomCtor.prototype` on the result (test262 `ctor-custom-prototype.js`),
/// falling back to the invoked closure's own `"prototype"` when `new.target`
/// is absent (a bare `new Intl.X()`) or its `prototype` isn't an object.
pub(super) fn constructor_target_prototype(closure: *const ClosureHeader) -> f64 {
    let new_target = crate::object::js_new_target_get();
    let bits = new_target.to_bits();
    if (bits & !POINTER_MASK) == POINTER_TAG {
        let raw = (bits & POINTER_MASK) as usize;
        if raw != 0 {
            let key = js_string_from_bytes(b"prototype".as_ptr(), b"prototype".len() as u32);
            let proto = js_object_get_field_by_name_f64(raw as *const ObjectHeader, key);
            if JSValue::from_bits(proto.to_bits()).is_pointer() {
                return proto;
            }
        }
    }
    crate::closure::closure_get_dynamic_prop(closure as usize, "prototype")
}

/// `Intl.<X>` is `[[Construct]]`-only per ECMA-402 (unlike the legacy
/// factory-pattern `NumberFormat`/`DateTimeFormat`/`Collator`): a bare call
/// or `.call(obj)` must throw a `TypeError` rather than silently `new`-ing.
pub(super) fn require_new_target(name: &str) {
    if crate::object::js_new_target_get().to_bits() == crate::value::TAG_UNDEFINED {
        super::throw_type_error(&format!("Constructor Intl.{name} requires 'new'"));
    }
}

/// The immediate `[[Prototype]]` pointer of an object at `obj_ptr`, resolved
/// across Perry's two prototype-recording mechanisms:
///   * the static-prototype side table (`object_set_static_prototype`, used by
///     Intl instances and `Object.setPrototypeOf`), and
///   * a synthetic `class_id` → `CLASS_PROTOTYPE_OBJECTS` entry (used by
///     `Object.create(proto)`).
/// Returns `None` when the object has its default `Object.prototype` (or a null
/// prototype), which is never a service constructor's own `.prototype`.
fn object_proto_ptr(obj_ptr: usize) -> Option<usize> {
    if let Some(proto_bits) = crate::object::prototype_chain::object_static_prototype(obj_ptr) {
        if (proto_bits & !POINTER_MASK) == POINTER_TAG {
            let p = (proto_bits & POINTER_MASK) as usize;
            return (p != 0).then_some(p);
        }
        return None;
    }
    let class_id = unsafe {
        let obj = obj_ptr as *const ObjectHeader;
        if !crate::object::is_valid_obj_ptr(obj as *const u8) {
            return None;
        }
        (*obj).class_id
    };
    if class_id == 0 {
        return None;
    }
    let proto = crate::object::class_prototype_object(class_id);
    (!proto.is_null()).then_some(proto as usize)
}

/// True when `receiver`'s prototype chain contains `Ctor.prototype` — the
/// `OrdinaryHasInstance(constructor, receiver)` test used by ChainNumberFormat /
/// ChainDateTimeFormat. Walks the resolved prototype chain (bounded to avoid a
/// cyclic-proto hang).
fn receiver_on_constructor_chain(receiver: f64, closure: *const ClosureHeader) -> bool {
    let ctor_proto = crate::closure::closure_get_dynamic_prop(closure as usize, "prototype");
    let ctor_proto_bits = ctor_proto.to_bits();
    if (ctor_proto_bits & !POINTER_MASK) != POINTER_TAG {
        return false;
    }
    let ctor_proto_ptr = (ctor_proto_bits & POINTER_MASK) as usize;
    if ctor_proto_ptr == 0 {
        return false;
    }
    let mut cur = (receiver.to_bits() & POINTER_MASK) as usize;
    for _ in 0..64 {
        match object_proto_ptr(cur) {
            Some(proto_ptr) => {
                if proto_ptr == ctor_proto_ptr {
                    return true;
                }
                cur = proto_ptr;
            }
            None => return false,
        }
    }
    false
}

/// ChainNumberFormat / ChainDateTimeFormat (ECMA-402 normative-optional): when a
/// legacy Intl service constructor is invoked as a *plain function* (no `new`)
/// whose `this` is an object on the constructor's prototype chain, stash the
/// freshly-built internal `instance` under `%Intl%.[[FallbackSymbol]]` on `this`
/// (a non-writable / non-enumerable / non-configurable data property) and return
/// `this`. Returns `Some(this)` when the chain applied, else `None` so the caller
/// returns the plain instance.
///
/// V8/Node implement this for `NumberFormat` and `DateTimeFormat` only —
/// `Intl.Collator` ignores its this-value and always returns a fresh object
/// (test262 `Collator/this-value-ignored.js`), so the caller must not invoke
/// this for the Collator kind.
pub(super) fn chain_legacy_constructed(
    closure: *const ClosureHeader,
    instance: f64,
) -> Option<f64> {
    // Only the legacy path: a `new`-invocation (new.target present) always
    // returns the fresh instance.
    if crate::object::js_new_target_get().to_bits() != crate::value::TAG_UNDEFINED {
        return None;
    }
    let this_value = crate::object::js_implicit_this_get();
    let this_bits = this_value.to_bits();
    // `this` must be an Object (pointer-tagged, valid heap object).
    if (this_bits & !POINTER_MASK) != POINTER_TAG {
        return None;
    }
    let this_ptr = (this_bits & POINTER_MASK) as usize;
    if this_ptr == 0 || !crate::object::is_valid_obj_ptr(this_ptr as *const u8) {
        return None;
    }
    if !receiver_on_constructor_chain(this_value, closure) {
        return None;
    }
    let fallback_sym = crate::symbol::intl_legacy_constructed_symbol();
    unsafe {
        crate::symbol::js_object_set_symbol_property(this_value, fallback_sym, instance);
    }
    let sym_key = (fallback_sym.to_bits() & POINTER_MASK) as usize;
    crate::symbol::set_symbol_property_attrs(
        this_ptr,
        sym_key,
        PropertyAttrs::new(false, false, false),
    );
    Some(this_value)
}
