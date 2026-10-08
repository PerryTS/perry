use super::*;
use std::collections::HashMap;

const CLASS_LEXICAL_BINDING_KEY: &str = "#<perry:private-class-lexical-binding>";

/// Read/write storage for the outer mutable binding introduced by a class
/// declaration.  The class body's same-spelled inner binding is lowered
/// directly to its ClassRef and never reaches these helpers.
#[no_mangle]
pub extern "C" fn js_class_lexical_binding_get(class_ref: f64) -> f64 {
    let Some(class_id) = class_ref_id(class_ref) else {
        return class_ref;
    };
    class_own_static_field_value(class_id, CLASS_LEXICAL_BINDING_KEY).unwrap_or(class_ref)
}

#[no_mangle]
pub extern "C" fn js_class_lexical_binding_set(class_ref: f64, value: f64) -> f64 {
    if let Some(class_id) = class_ref_id(class_ref) {
        class_dynamic_prop_root_store(class_id, CLASS_LEXICAL_BINDING_KEY, value);
    }
    value
}

/// Register a static field value on a class so `Cls.field` (when `Cls` is
/// accessed via dynamic dispatch — e.g. through an Any-typed local) finds
/// the value via the runtime path. Codegen calls this at module init for
/// every static field initializer in addition to writing the value to the
/// per-field module global. `global_slot` binds those two storage views so a
/// later computed/member write can update the cell used by direct reads.
/// Refs #420 / #618 followup / #9526. Static-field values are stored in
/// CLASS_DYNAMIC_PROPS keyed by class_id.
#[no_mangle]
pub unsafe extern "C" fn js_class_register_static_field(
    class_id: u32,
    name_ptr: *const u8,
    name_len: usize,
    value: f64,
    global_slot: *mut f64,
) {
    if class_id == 0 || name_ptr.is_null() || name_len == 0 {
        return;
    }
    let Ok(name) = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) else {
        return;
    };
    class_register_declared_static_global_slot(class_id, name, global_slot);
    class_dynamic_prop_root_store(class_id, name, value);
    // DefineField: a static field is an ordinary writable, enumerable,
    // configurable own property — also when it replaces the class's
    // intrinsic `name` / `length`.
    crate::object::class_value::note_static_field_defined(class_id, name);
}

/// Define a static PRIVATE field as an `ENTRY_PRIVATE` slot of the class
/// function's bag. It never has a corresponding public property or alias.
#[no_mangle]
pub unsafe extern "C" fn js_class_register_static_private_field(
    class_id: u32,
    name_ptr: *const u8,
    name_len: usize,
    value: f64,
    _global_slot: *mut f64,
) {
    if class_id == 0 || name_ptr.is_null() || name_len == 0 {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let receiver = scope.root_nanbox_f64(crate::object::class_constructor_ref_value(class_id));
    let key = crate::string::js_string_from_bytes(name_ptr, name_len as u32);
    crate::object::field_get_set::define_static_private_field(
        receiver.get_nanbox_f64(),
        key,
        value.get_nanbox_f64(),
    );
}

/// Define a static PRIVATE field of a fresh class evaluation on its class
/// object, directly in the private namespace of its own shape.
#[no_mangle]
pub unsafe extern "C" fn js_class_object_define_static_private(
    class_object: *mut crate::object::ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) {
    if key.is_null() {
        return;
    }
    crate::object::field_get_set::define_static_private_field(
        crate::value::js_nanbox_pointer(class_object as i64),
        key,
        value,
    );
}

/// Read a computed instance-field key resolved at ClassDefinitionEvaluation.
/// Fresh class values carry the hidden slot on their heap class object; plain
/// class references use the class-id static side table.
#[no_mangle]
pub unsafe extern "C" fn js_class_computed_field_key(
    receiver: f64,
    class_id: u32,
    name_ptr: *const u8,
    name_len: usize,
) -> f64 {
    if name_ptr.is_null() || name_len == 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if let Some(owner) = crate::object::private_evaluation_brand_value(receiver) {
        let value = crate::object::js_object_get_own_field_or_undef(owner, name_ptr, name_len);
        if value.to_bits() != crate::value::TAG_UNDEFINED {
            return value;
        }
    }
    let bytes = std::slice::from_raw_parts(name_ptr, name_len);
    let Ok(name) = std::str::from_utf8(bytes) else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    class_own_static_field_value(class_id, name)
        .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED))
}

// Production codegen reads this fail-closed all-method byte and the scoped
// table below before entering a guarded direct-method arm. Keep the test state
// per-thread so one mutation test cannot poison unrelated tests running in
// parallel; generated programs link the non-test symbols below.
#[cfg(not(test))]
#[no_mangle]
pub static PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED: std::sync::atomic::AtomicU8 =
    std::sync::atomic::AtomicU8::new(0);

/// Sticky per-method invalidation bytes for compiler-emitted direct-method
/// guards. Prototype writes always have a property name, so they only need to
/// retire guards for that name. The low 16 bits of the name's FNV-1a hash
/// select a byte; collisions conservatively retire additional names.
pub(crate) const CLASS_PROTOTYPE_METHOD_GUARD_SLOT_COUNT: usize = 1 << 16;
pub(crate) const CLASS_PROTOTYPE_METHOD_GUARD_SLOT_MASK: u64 =
    (CLASS_PROTOTYPE_METHOD_GUARD_SLOT_COUNT - 1) as u64;

#[cfg(not(test))]
#[no_mangle]
pub static PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD: [std::sync::atomic::AtomicU8;
    CLASS_PROTOTYPE_METHOD_GUARD_SLOT_COUNT] =
    [const { std::sync::atomic::AtomicU8::new(0) }; CLASS_PROTOTYPE_METHOD_GUARD_SLOT_COUNT];

#[cfg(test)]
per_test_global! {
    pub(crate) static CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);
    pub(crate) static CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD:
        std::sync::RwLock<std::collections::HashSet<u16>> =
        std::sync::RwLock::new(std::collections::HashSet::new());
}

#[inline]
pub(crate) fn class_prototype_method_guard_slot(name: &str) -> u32 {
    (super::super::key_bytes_hash(name.as_ptr(), name.len())
        & CLASS_PROTOTYPE_METHOD_GUARD_SLOT_MASK) as u32
}

#[inline]
fn retire_prototype_dependent_caches() {
    // #7480: prototype surgery retires element-shape proofs.
    crate::array::invalidate_all_element_shapes();
    // #7769: method-dispatch caches are keyed by VTABLE_GEN.
    VTABLE_GEN.fetch_add(1, std::sync::atomic::Ordering::Release);
}

pub(crate) fn invalidate_class_prototype_fast_guards_for_method(name: &str) {
    let slot = class_prototype_method_guard_slot(name) as usize;
    #[cfg(not(test))]
    PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD[slot]
        .store(1, std::sync::atomic::Ordering::Release);
    #[cfg(test)]
    CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD
        .write()
        .unwrap()
        .insert(slot as u16);
    retire_prototype_dependent_caches();
}

/// A user operation (`Object.setPrototypeOf(C.prototype, X)`,
/// `C.prototype.__proto__ = X`) replaced the `[[Prototype]]` of `proto`.
///
/// When `proto` is a declared class's prototype object, every member that
/// class's instances inherited from its DECLARED ancestors is off their chain
/// from now on, and whatever `X` carries is on it. The runtime walks over
/// declared members stop at the relinked class
/// (`instance_chain_parent_class_id`); what remains are the resolutions made
/// before the relink or ahead of time:
///
/// Compiler-emitted arms compare every holder ShapeId. The historical byte
/// writes remain until S7, but no call guard reads them.
/// The runtime's by-name method calls need nothing: they read the
/// prototype chain's shapes (`native_call_method::class_holder`), and the
/// relink restamps `proto`. Neither do the receiver-word site memos: the
/// relink restamps `proto`'s shape, which their hop facts compare.
///
/// # Safety
/// `proto` must point to a live, meta-capable object.
pub(crate) unsafe fn class_prototype_relinked(proto: *mut crate::object::ObjectHeader) {
    let cid = (*proto).class_id;
    if cid == 0 || super::class_decl_prototype_object(cid) != proto {
        return;
    }
    super::super::prototype_chain::note_class_chain_relinked();
    let mut names: Vec<String> = Vec::new();
    if let Ok(registry) = super::CLASS_VTABLE_REGISTRY.read() {
        if let Some(reg) = registry.as_ref() {
            let mut cur = cid;
            for _ in 0..32 {
                match super::get_parent_class_id(cur) {
                    Some(pid) if pid != 0 && pid != cur => cur = pid,
                    _ => break,
                }
                if let Some(vt) = reg.get(&cur) {
                    names.extend(vt.methods.keys().cloned());
                    names.extend(vt.accessors.keys().cloned());
                }
            }
        }
    }
    for name in &names {
        let slot = class_prototype_method_guard_slot(name) as usize;
        #[cfg(not(test))]
        PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD[slot]
            .store(1, std::sync::atomic::Ordering::Release);
        #[cfg(test)]
        CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD
            .write()
            .unwrap()
            .insert(slot as u16);
    }
    retire_prototype_dependent_caches();
}

pub(crate) fn invalidate_class_prototype_fast_guards() {
    #[cfg(not(test))]
    PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED.store(1, std::sync::atomic::Ordering::Release);
    #[cfg(test)]
    CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED.store(true, std::sync::atomic::Ordering::Release);
    // Preserve the historical byte writer until S7 and retire runtime caches.
    retire_prototype_dependent_caches();
}

/// Can a new `[[Prototype]]` on the object at `obj_ptr` change what a
/// compiler-emitted direct-method arm resolved?
///
/// Those arms call a body the compiler resolved along a declared class's
/// `extends` chain, for a receiver whose exact `(class_id, ShapeId)` the arm
/// compares on every call. Their resolution can only move when the new link
/// sits on such a chain or on such a receiver:
///
/// * a class instance (a real class id): conservatively retired, although its
///   ShapeId already names the new prototype;
/// * a declared class's prototype object: [`class_prototype_relinked`]
///   retires the inherited names precisely, and this keeps the all-names latch
///   on top;
/// * an object perry cannot classify (not meta-capable, not regular): retired.
///
/// Every other target — an object literal, an `Object.create` result, a plain
/// function's `.prototype` (`util.inherits`, `setPrototypeOf(Sub.prototype,
/// Base.prototype)`; registered under the function's synthetic class id, which
/// no compiled arm names), a dictionary built with `__proto__` — is on no
/// declared class chain. A class whose chain later reaches it (`class X extends Sub`)
/// resolves no inherited body statically through a function base, and an
/// instance that later takes it as its prototype is re-stamped by that link.
/// Retiring every direct arm in the process for these turned one
/// `util.inherits` in any dependency into a 3.7x tax on every `this.m()`
/// (#10504).
///
/// # Safety
/// `obj_ptr` must be a heap address the caller already validated as an object.
pub(crate) unsafe fn prototype_relink_may_retarget_direct_arms(obj_ptr: usize) -> bool {
    let Some(obj) = crate::object::prototype_chain::meta_capable_object(obj_ptr) else {
        return true;
    };
    if !crate::object::object_is_regular(obj) {
        return true;
    }
    let class_id = (*obj).class_id;
    if class_id != 0 && !super::is_anon_shape_class_id(class_id) {
        return true;
    }
    super::class_id_for_decl_prototype_object(obj_ptr).is_some()
}

/// The cache retirements of [`invalidate_class_prototype_fast_guards`]
/// without its all-names latch: for a prototype relink that
/// [`prototype_relink_may_retarget_direct_arms`] proves cannot move a compiled
/// direct arm. The runtime's `(class, name)` dispatch caches and element-shape
/// proofs still restart.
pub(crate) fn retire_prototype_caches_without_direct_arms() {
    retire_prototype_dependent_caches();
}

/// Compatibility entry points for HIR assembled by embedders. The prototype
/// is read before the value is stored; its ordinary [[Set]] owns the mutation.
pub(crate) fn class_prototype_set(class_id: u32, name: String, value_bits: u64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_u64(value_bits);
    let proto = class_prototype_object(class_id);
    let proto = if proto.is_null() {
        class_decl_prototype_value(class_id)
    } else {
        crate::value::js_nanbox_pointer(proto as i64)
    };
    let proto = scope.root_nanbox_f64(proto);
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let key = crate::value::js_nanbox_string(key as i64);
    crate::proxy::js_put_value_set(
        proto.get_nanbox_f64(),
        key,
        value.get_nanbox_f64(),
        proto.get_nanbox_f64(),
        1,
    );
}

#[no_mangle]
pub unsafe extern "C" fn js_register_prototype_method(
    class_id: u32,
    name_ptr: *const u8,
    name_len: usize,
    value: f64,
) {
    if name_ptr.is_null() {
        return;
    }
    if let Ok(name) = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
        class_prototype_set(class_id, name.to_string(), value.to_bits());
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_get_function_prototype_method(
    func_value: f64,
    name_ptr: *const u8,
    name_len: usize,
) -> f64 {
    if name_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto = scope.root_nanbox_f64(js_function_prototype_value_for_read(func_value));
    let key = crate::string::js_string_from_bytes(name_ptr, name_len as u32);
    super::super::field_get_set::js_object_get_field_by_name_f64(
        proto.get_nanbox_f64().to_bits() as *const ObjectHeader,
        key,
    )
}

#[no_mangle]
pub unsafe extern "C" fn js_register_function_prototype_method(
    func_value: f64,
    name_ptr: *const u8,
    name_len: usize,
    value: f64,
) -> u32 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let func = scope.root_nanbox_f64(func_value);
    let value = scope.root_nanbox_f64(value);
    let proto = scope.root_nanbox_f64(js_function_prototype_value_for_read(func.get_nanbox_f64()));
    if !name_ptr.is_null() {
        let key = crate::string::js_string_from_bytes(name_ptr, name_len as u32);
        crate::proxy::js_put_value_set(
            proto.get_nanbox_f64(),
            crate::value::js_nanbox_string(key as i64),
            value.get_nanbox_f64(),
            proto.get_nanbox_f64(),
            1,
        );
    }
    function_class_id(func.get_nanbox_f64())
}

/// Get-or-allocate a synthetic class id keyed by a function value's
/// NaN-boxed bits. Used by `js_register_function_prototype_method` (HIR
/// "Func.prototype.x = fn" recogniser) and `js_new_function_construct`
/// (HIR "new Func(args)" allocator) so both sides agree on the same id
/// — the instance's `(*obj).class_id` lands in the same bucket the
/// method registration stored against. Returns 0 if `func_value` isn't a
/// POINTER_TAG'd value (callable shape requirement).
pub(crate) fn synthetic_class_id_for_function(func_value: f64) -> u32 {
    let func_bits = func_value.to_bits();
    // Require a verified closure shape so we don't store arbitrary
    // POINTER_TAG'd pointers (arrays, objects, etc. all share the tag)
    // in `FUNCTION_CLASS_IDS`. The bits-as-key invariant only makes
    // sense for callable values that produced a stable singleton
    // closure pointer.
    if !is_callable_function_value(func_value) {
        return 0;
    }
    let existing = FUNCTION_CLASS_IDS.with(|table| {
        let read = table.read().unwrap();
        if let Some(map) = read.as_ref() {
            if let Some(&existing) = map.get(&func_bits) {
                return Some(existing);
            }
        }
        None
    });
    if let Some(existing) = existing {
        return existing;
    }
    let new_cid = super::prototype_objects::alloc_synthetic_class_id();
    FUNCTION_CLASS_IDS.with(|table| {
        let mut write = table.write().unwrap();
        if write.is_none() {
            *write = Some(HashMap::new());
        }
        write.as_mut().unwrap().insert(func_bits, new_cid);
    });
    unsafe { js_register_class_id(new_cid) };
    new_cid
}
