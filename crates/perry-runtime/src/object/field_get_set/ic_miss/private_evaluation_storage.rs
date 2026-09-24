// Storage names for fresh ClassDefinitionEvaluations (#11163).

// A scalar identity survives moving GC without retaining its class object.
static NEXT_PRIVATE_EVALUATION_ID: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

fn private_storage_namespace_for(class_id: u32, receiver: Option<f64>) -> String {
    private_storage_namespace_for_owner(class_id, receiver, None)
}

fn private_storage_namespace_for_owner(
    class_id: u32,
    receiver: Option<f64>,
    owner: Option<u64>,
) -> String {
    let Some(brand) = owner
        .or_else(|| current_private_lexical_brand(class_id))
        .or_else(|| receiver.and_then(|value| private_evaluation_brand(value, class_id)))
    else {
        return class_id.to_string();
    };
    let object = JSValue::from_bits(brand).as_pointer::<ObjectHeader>();
    // object_meta_ensure roots the class across metadata allocation; callers
    // separately root the receiver and any other live operands.
    // A heap class object cannot also be a native decoder or Set: its
    // native_state word is available for this class-specific scalar payload.
    let id = unsafe {
        let meta = crate::object::object_meta_ensure(object as *mut ObjectHeader);
        if (*meta).native_state == 0 {
            (*meta).native_state =
                NEXT_PRIVATE_EVALUATION_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        (*meta).native_state
    };
    format!("{class_id}:@{id}")
}

fn private_instance_value_name(
    class_id: u32,
    field_name: &str,
    receiver: f64,
    owner: Option<u64>,
) -> String {
    format!(
        "#<perry:private-value:{}:{field_name}>",
        private_storage_namespace_for_owner(class_id, Some(receiver), owner)
    )
}

/// The compiler's template key is a request for a lexical private name.
/// Qualified storage keys deliberately do not match this parser again.
fn private_value_request(key: *const crate::StringHeader) -> Option<(u32, String)> {
    let key = unsafe { super::super::has_own_helpers::str_from_string_header(key) }?;
    let rest = key
        .strip_prefix("#<perry:private-value:")?
        .strip_suffix('>')?;
    let (class_id, name) = rest.split_once(':')?;
    if !name.starts_with('#') {
        return None;
    }
    Some((class_id.parse().ok()?, name.to_owned()))
}

fn private_evaluation_field_get(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<f64> {
    let (class_id, name) = private_value_request(key)?;
    let owner = take_private_field_owner(class_id, &name, false);
    let _owner = PrivateHintBrandScope::new(owner);
    let receiver = private_member_receiver(obj);
    if super::super::class_registry::is_class_object_value(receiver)
        || super::super::native_module::class_ref_id(receiver).is_some()
        || (current_private_lexical_brand(class_id).is_none()
            && private_evaluation_brand(receiver, class_id).is_none())
    {
        return None;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let name = private_instance_value_name(class_id, &name, receiver.get_nanbox_f64(), owner);
    Some(crate::object::js_object_get_own_field_or_undef(
        receiver.get_nanbox_f64(),
        name.as_ptr(),
        name.len(),
    ))
}

fn private_evaluation_field_set(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) -> bool {
    let Some((class_id, name)) = private_value_request(key) else {
        return false;
    };
    let owner = take_private_field_owner(class_id, &name, true);
    let _owner = PrivateHintBrandScope::new(owner);
    let receiver = private_member_receiver(obj);
    if super::super::class_registry::is_class_object_value(receiver)
        || super::super::native_module::class_ref_id(receiver).is_some()
        || (current_private_lexical_brand(class_id).is_none()
            && private_evaluation_brand(receiver, class_id).is_none())
    {
        return false;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let value = scope.root_nanbox_f64(value);
    let name = private_instance_value_name(class_id, &name, receiver.get_nanbox_f64(), owner);
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let obj = JSValue::from_bits(receiver.get_nanbox_f64().to_bits()).as_pointer::<ObjectHeader>();
    js_object_set_field_by_name(obj as *mut ObjectHeader, key, value.get_nanbox_f64());
    true
}

/// An instance can contain several evaluations of the same template. Search
/// the pinned heritage by identity, while constructors themselves stay exact.
fn private_evaluation_brand_is(obj: f64, expected: u64) -> bool {
    if super::super::class_registry::is_class_object_value(obj) {
        return obj.to_bits() == expected;
    }
    let Some(mut current) = private_evaluation_brand_value(obj) else {
        return false;
    };
    for _ in 0..32 {
        if current.to_bits() == expected {
            return true;
        }
        let object = JSValue::from_bits(current.to_bits()).as_pointer::<ObjectHeader>();
        let Some(parent) = super::super::class_registry::class_object_pinned_parent(object) else {
            return false;
        };
        if !super::super::class_registry::is_class_object_value(parent) {
            return false;
        }
        current = parent;
    }
    false
}

fn private_access_owner(brand_owner: f64, class_id: u32) -> Option<u64> {
    current_private_lexical_brand(class_id).or_else(|| {
        let owner = if super::super::class_registry::is_class_object_value(brand_owner) {
            super::super::static_private_owner_current().unwrap_or(brand_owner)
        } else {
            brand_owner
        };
        private_evaluation_brand(owner, class_id)
    })
}

fn take_private_field_owner(class_id: u32, name: &str, is_write: bool) -> Option<u64> {
    PRIVATE_MEMBER_ACCESS_HINTS.with(|hints| {
        let mut hints = hints.borrow_mut();
        let index = hints.iter().rposition(|hint| {
            hint.class_id == class_id
                && hint.name == name
                && hint.kind == 0
                && !hint.is_static
                && hint.is_write == is_write
        })?;
        hints.remove(index).brand_owner
    })
}

pub(crate) struct PrivateHintBrandScope(usize);
impl PrivateHintBrandScope {
    pub(crate) fn new(owner: Option<u64>) -> Self {
        let depth = private_lexical_brand_stack_savepoint();
        if let Some(owner) = owner {
            private_lexical_brand_push(f64::from_bits(owner));
        }
        Self(depth)
    }
}
impl Drop for PrivateHintBrandScope {
    fn drop(&mut self) {
        private_lexical_brand_stack_restore(self.0);
    }
}
