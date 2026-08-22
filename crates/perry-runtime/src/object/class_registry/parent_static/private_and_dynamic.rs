pub(crate) fn class_own_symbol_method(
    class_id: u32,
    sym_key: usize,
    is_static: bool,
) -> Option<(usize, u32, bool)> {
    CLASS_SYMBOL_METHODS.with(|table| {
        table
            .read()
            .ok()?
            .as_ref()?
            .get(&(class_id, sym_key, is_static))
            .copied()
    })
}

pub(crate) fn class_own_symbol_accessor_ptrs(
    class_id: u32,
    sym_key: usize,
    is_static: bool,
) -> Option<(usize, usize)> {
    CLASS_SYMBOL_ACCESSORS.with(|table| {
        table
            .read()
            .ok()?
            .as_ref()?
            .get(&(class_id, sym_key, is_static))
            .copied()
    })
}

fn dynamic_static_accessor_key(name: &str) -> String {
    let mut key = String::with_capacity(name.len() + 24);
    key.push('\0');
    key.push_str("perry:class-static:");
    key.push_str(name);
    key
}

fn dynamic_static_accessor_owner(class_id: u32) -> usize {
    let value = class_decl_prototype_value(class_id);
    let bits = value.to_bits();
    if (bits >> 48) == 0x7FFD {
        (bits & crate::value::POINTER_MASK) as usize
    } else {
        0
    }
}

/// Store an accessor installed dynamically on a class constructor through
/// `Object.defineProperty(C, key, { get, set })`. Class constructors are
/// immediate ClassRef values rather than heap objects, so keep the rooted
/// accessor descriptor on the class's materialized prototype under an
/// internal key; the public static lookup paths consult it by class id.
pub(crate) fn register_class_dynamic_static_accessor(
    class_id: u32,
    name: &str,
    get_bits: u64,
    set_bits: u64,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let get = scope.root_nanbox_u64(get_bits);
    let set = scope.root_nanbox_u64(set_bits);
    let owner = dynamic_static_accessor_owner(class_id);
    if owner == 0 {
        return;
    }
    crate::object::set_accessor_descriptor(
        owner,
        dynamic_static_accessor_key(name),
        crate::object::AccessorDescriptor {
            get: get.get_nanbox_u64(),
            set: set.get_nanbox_u64(),
        },
    );
}

pub(crate) unsafe fn class_dynamic_static_accessor_getter_value(
    class_id: u32,
    name: &str,
    receiver: f64,
) -> Option<f64> {
    let owner = dynamic_static_accessor_owner(class_id);
    let descriptor = (owner != 0)
        .then(|| crate::object::get_accessor_descriptor(owner, &dynamic_static_accessor_key(name)))
        .flatten()?;
    if descriptor.get == 0 {
        return Some(f64::from_bits(crate::value::TAG_UNDEFINED));
    }
    Some(f64::from_bits(
        crate::object::invoke_accessor_getter(descriptor.get, receiver).bits(),
    ))
}

/// `Some(true)` means a setter was invoked, `Some(false)` means an accessor
/// exists but has no setter, and `None` means this class has no such dynamic
/// accessor.
pub(crate) unsafe fn class_dynamic_static_accessor_setter_apply(
    class_id: u32,
    name: &str,
    receiver: f64,
    value: f64,
) -> Option<bool> {
    let owner = dynamic_static_accessor_owner(class_id);
    let descriptor = (owner != 0)
        .then(|| crate::object::get_accessor_descriptor(owner, &dynamic_static_accessor_key(name)))
        .flatten()?;
    if descriptor.set == 0 {
        return Some(false);
    }
    crate::object::invoke_accessor_setter(descriptor.set, receiver, value);
    Some(true)
}

/// Invoke an instance-private getter on its lexical declaring class. Unlike
/// ordinary public accessor lookup, private names are not inherited and must
/// not be shadowed by a public string property with the same spelling.
pub(crate) unsafe fn class_private_instance_getter_value(
    class_id: u32,
    name: &str,
    receiver: f64,
) -> Option<f64> {
    let guard = CLASS_VTABLE_REGISTRY.read().ok()?;
    let vtable = guard.as_ref()?.get(&class_id)?;
    let &getter = vtable.getters.get(name)?;
    if getter == 0 {
        return Some(f64::from_bits(crate::value::TAG_UNDEFINED));
    }
    let f: extern "C" fn(f64) -> f64 = std::mem::transmute(getter);
    Some(f(receiver))
}

/// Invoke an instance-private setter on its lexical declaring class.
pub(crate) unsafe fn class_private_instance_setter_apply(
    class_id: u32,
    name: &str,
    receiver: f64,
    value: f64,
) -> bool {
    let guard = match CLASS_VTABLE_REGISTRY.read() {
        Ok(guard) => guard,
        Err(_) => return false,
    };
    let Some(vtable) = guard.as_ref().and_then(|registry| registry.get(&class_id)) else {
        return false;
    };
    let Some(&setter) = vtable.setters.get(name) else {
        return false;
    };
    if setter != 0 {
        let f: extern "C" fn(f64, f64) -> f64 = std::mem::transmute(setter);
        let _ = f(receiver, value);
    }
    true
}

pub(crate) unsafe fn call_private_static_method_for_owner(
    owner_class_id: u32,
    name: &str,
    this_value: f64,
    private_brand: f64,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    let (func_ptr, param_count, has_rest) = CLASS_STATIC_METHODS
        .read()
        .ok()?
        .as_ref()?
        .get(&owner_class_id)?
        .get(name)
        .copied()?;
    let scope = crate::gc::RuntimeHandleScope::new();
    let this_value = scope.root_nanbox_f64(this_value);
    let private_brand = scope.root_nanbox_f64(private_brand);
    let previous_this = crate::object::js_implicit_this_set(this_value.get_nanbox_f64());
    crate::object::static_private_owner_push(private_brand.get_nanbox_f64());
    crate::object::private_lexical_brand_push(private_brand.get_nanbox_f64());
    crate::object::static_this_arm_if_unarmed(this_value.get_nanbox_f64());
    let result = call_registered_static_method(func_ptr, args_ptr, args_len, param_count, has_rest);
    crate::object::static_this_disarm();
    crate::object::private_lexical_brand_pop();
    crate::object::static_private_owner_pop();
    crate::object::js_implicit_this_set(previous_this);
    Some(result)
}
