// Storage names for fresh ClassDefinitionEvaluations (#11163).

// Identity 0 belongs exclusively to runtime-intrinsic private names.
// A template that needs no fresh evaluation uses a separate scalar namespace.
const PRIVATE_TEMPLATE_EVALUATION_ID: u64 = u64::MAX;

// A scalar identity survives moving GC without retaining its class object.
static NEXT_PRIVATE_EVALUATION_ID: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

include!("private_storage_cache.rs");

// Static fields belong to the constructor itself. The lexical brand guard
// distinguishes evaluations, so their own storage key needs no evaluation id.
fn static_private_field_key(class_id: u32, name: &str) -> std::rc::Rc<PrivateStorageKey> {
    private_storage_key_by_id(
        class_id,
        PRIVATE_TEMPLATE_EVALUATION_ID,
        intern_private_name(name.as_bytes()).unwrap(),
    )
}

/// Define the private entry directly, without first creating a public property
/// of the same spelling. Namespace-aware attribute edits cannot convert one
/// namespace into the other.
pub(crate) unsafe fn define_static_private_field(
    receiver: f64,
    key: *const crate::StringHeader,
    value: f64,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let value = scope.root_nanbox_f64(value);
    let spelling = super::super::has_own_helpers::str_from_string_header(key)
        .expect("static private storage key")
        .to_owned();
    let storage = PrivateStorageKey {
        spelling,
        slot: std::cell::Cell::new(None),
    };
    let addr = crate::value::js_nanbox_get_pointer(receiver.get_nanbox_f64()) as usize;
    if crate::closure::is_closure_ptr(addr) {
        crate::closure::props::bag_ensure(addr);
    }
    let holder = private_element_holder(receiver.get_nanbox_f64()).expect("private field holder");
    let holder = scope.root_raw_mut_ptr(holder);
    holder.with_mut_ptr::<ObjectHeader, _>(|holder| {
        crate::object::key_attrs::apply_edits(
            holder,
            &[crate::object::key_attrs::AttrsEdit::Private(
                storage.as_bytes(),
            )],
        );
    });
    assert!(storage.set_cached(receiver.get_nanbox_f64(), value.get_nanbox_f64()));
}

/// A compiled static PrivateGet, with its identity and initialization checks.
#[no_mangle]
pub extern "C" fn js_private_static_field_get(
    receiver: f64,
    brand_owner: f64,
    class_id: u32,
    name_ptr: *const u8,
    name_len: u32,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let brand_owner = scope.root_nanbox_f64(brand_owner);
    let name = unsafe { std::slice::from_raw_parts(name_ptr, name_len as usize) };
    private_guard_checked(
        receiver.get_nanbox_f64(),
        brand_owner.get_nanbox_f64(),
        class_id,
        name,
        0,
        2,
        false,
    );
    let name = intern_private_name(name).unwrap();
    static_private_field_key(class_id, name).get(receiver.get_nanbox_f64())
}

/// A compiled static PrivateSet: called after evaluating the right-hand side.
#[no_mangle]
pub extern "C" fn js_private_static_field_set(
    receiver: f64,
    brand_owner: f64,
    class_id: u32,
    name_ptr: *const u8,
    name_len: u32,
    value: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let brand_owner = scope.root_nanbox_f64(brand_owner);
    let value = scope.root_nanbox_f64(value);
    let name = unsafe { std::slice::from_raw_parts(name_ptr, name_len as usize) };
    private_guard_checked(
        receiver.get_nanbox_f64(),
        brand_owner.get_nanbox_f64(),
        class_id,
        name,
        0,
        3,
        false,
    );
    let name = intern_private_name(name).unwrap();
    assert!(static_private_field_key(class_id, name)
        .set_cached(receiver.get_nanbox_f64(), value.get_nanbox_f64()));
    value.get_nanbox_f64()
}

fn private_storage_evaluation_id(class_id: u32, receiver: Option<f64>, owner: Option<u64>) -> u64 {
    let Some(brand) = owner
        .or_else(|| current_private_lexical_brand(class_id))
        .or_else(|| receiver.and_then(|value| private_evaluation_brand(value, class_id)))
    else {
        return PRIVATE_TEMPLATE_EVALUATION_ID;
    };
    let object = JSValue::from_bits(brand).as_pointer::<ObjectHeader>();
    unsafe {
        let meta = (*object).meta;
        if !meta.is_null() && (*meta).native_state != 0 {
            return (*meta).native_state;
        }
        // Metadata allocation can move the class; object_meta_ensure roots it.
        let meta = crate::object::object_meta_ensure(object as *mut ObjectHeader);
        let id = NEXT_PRIVATE_EVALUATION_ID
            .try_update(
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
                |id| id.checked_add(1).filter(|next| *next != PRIVATE_TEMPLATE_EVALUATION_ID),
            )
            .expect("private evaluation identity exhausted");
        (*meta).native_state = id;
        id
    }
}

fn private_instance_value_name(
    class_id: u32,
    field_name: &str,
    receiver: f64,
    owner: Option<u64>,
) -> std::rc::Rc<PrivateStorageKey> {
    private_storage_key(class_id, Some(receiver), owner, field_name)
}

/// The compiler's template key is a request for a lexical private name.
/// Qualified storage keys deliberately do not match this parser again.
///
/// The name borrows `key`'s bytes, so it is only valid until the next
/// allocation: callers reject the common single-evaluation case first
/// (nothing on that path allocates) and copy the name before going further
/// (#10501 — the owned copy used to be made on every IC-miss access).
unsafe fn private_value_request<'a>(key: *const crate::StringHeader) -> Option<(u32, &'a str)> {
    const PREFIX: &[u8] = b"#<perry:private-value:";
    if key.is_null() || ((*key).byte_len as usize) <= PREFIX.len() {
        return None;
    }
    // Reject an ordinary `#`-prefixed key on its bytes before validating it.
    let bytes = std::slice::from_raw_parts(crate::string::string_data(key), PREFIX.len());
    if bytes != PREFIX {
        return None;
    }
    let key = super::super::has_own_helpers::str_from_string_header(key)?;
    let rest = key
        .strip_prefix("#<perry:private-value:")?
        .strip_suffix('>')?;
    let (class_id, name) = rest.split_once(':')?;
    if !name.starts_with('#') {
        return None;
    }
    Some((class_id.parse().ok()?, name))
}

fn private_evaluation_field_get(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<f64> {
    let (class_id, name) = unsafe { private_value_request(key) }?;
    let receiver = private_member_receiver(obj);
    if private_static_receiver_is_constructor(receiver) {
        // Static fields use explicit PrivateGet, never a property string.
        return None;
    }
    if !private_template_may_be_evaluated(class_id) {
        let receiver = private_member_receiver(obj);
        let name = intern_private_name(name.as_bytes()).unwrap();
        return private_storage_key_by_id(class_id, PRIVATE_TEMPLATE_EVALUATION_ID, name).get_cached(receiver);
    }
    let owner = take_private_field_owner(class_id, name, false);
    let _owner = PrivateHintBrandScope::new(owner);
    let receiver = private_member_receiver(obj);
    if super::super::class_registry::is_class_object_value(receiver)
        || super::super::native_module::class_ref_id(receiver).is_some()
    {
        return None;
    }
    let name = intern_private_name(name.as_bytes()).unwrap();
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let name = private_instance_value_name(class_id, &name, receiver.get_nanbox_f64(), owner);
    Some(name.get(receiver.get_nanbox_f64()))
}

fn private_evaluation_field_set(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) -> bool {
    let Some((class_id, name)) = (unsafe { private_value_request(key) }) else {
        return false;
    };
    let receiver = private_member_receiver(obj);
    if private_static_receiver_is_constructor(receiver) {
        // Static fields use explicit PrivateSet, never a property string.
        return false;
    }
    if !private_template_may_be_evaluated(class_id) {
        let receiver = private_member_receiver(obj);
        let name = intern_private_name(name.as_bytes()).unwrap();
        return private_storage_key_by_id(class_id, PRIVATE_TEMPLATE_EVALUATION_ID, name).set_cached(receiver, value);
    }
    let owner = take_private_field_owner(class_id, name, true);
    let _owner = PrivateHintBrandScope::new(owner);
    let receiver = private_member_receiver(obj);
    if super::super::class_registry::is_class_object_value(receiver)
        || super::super::native_module::class_ref_id(receiver).is_some()
    {
        return false;
    }
    let name = intern_private_name(name.as_bytes()).unwrap();
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let value = scope.root_nanbox_f64(value);
    let name = private_instance_value_name(class_id, &name, receiver.get_nanbox_f64(), owner);
    if name.set_cached(receiver.get_nanbox_f64(), value.get_nanbox_f64()) {
        return true;
    }
    if !name.is_present(receiver.get_nanbox_f64()) {
        // A write never creates a property under a private name.
        throw_private_type_error(&format!(
            "Cannot write private member {} to an object whose class did not declare it",
            name.rsplit(':').next().unwrap_or("").trim_end_matches('>')
        ));
    }
    let key = crate::string::intern_ascii_literal(name.as_bytes());
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
