//! Shared `[[Construct]]`-only checks and `new.target`-aware prototype
//! resolution used by the Intl service constructors (#5835). Split out of
//! `intl.rs` to keep that file under the repository's 2,000-line gate.

use crate::closure::ClosureHeader;
use crate::object::{js_object_get_field_by_name_f64, ObjectHeader};
use crate::string::js_string_from_bytes;
use crate::value::JSValue;

/// `GetPrototypeFromConstructor(new.target, "%<Ctor>Prototype%")`: a
/// `Reflect.construct(Intl.X, args, CustomCtor)` call should install
/// `CustomCtor.prototype` on the result (test262 `ctor-custom-prototype.js`),
/// falling back to the invoked closure's own `"prototype"` when `new.target`
/// is absent (a bare `new Intl.X()`) or its `prototype` isn't an object.
pub(super) fn constructor_target_prototype(closure: *const ClosureHeader) -> f64 {
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
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
