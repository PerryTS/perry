// A private field's storage is an own key entry flagged `ENTRY_PRIVATE`
// (#11791), named by its qualified private name: a function of the scalar
// evaluation identity, never of a class-object address. Keep a bounded
// per-agent cache of those names so repeatedly creating class evaluations
// cannot retain unbounded strings or managed objects.

#[derive(Debug)]
struct PrivateStorageKey {
    spelling: String,
    // ShapeIds include key order, attributes and the live inline bound. No
    // object pointer is retained, and every hit reads the current slot value.
    slot: std::cell::Cell<Option<(u32, u32, u32)>>,
}

impl std::ops::Deref for PrivateStorageKey {
    type Target = str;
    fn deref(&self) -> &str {
        &self.spelling
    }
}

impl PrivateStorageKey {
    /// The receiver and the inline/overflow position of this field, when the
    /// receiver's shape lists it as a private field. `None` is "not present
    /// on an ordinary layout": an absent field, an unshaped receiver, or a key
    /// of this spelling that a program wrote as a property.
    #[inline]
    fn locate(&self, receiver: f64) -> Option<(*const ObjectHeader, u32, u32)> {
        let bits = receiver.to_bits();
        let addr = (bits & crate::value::POINTER_MASK) as usize;
        if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG
            || addr < crate::value::addr_class::HANDLE_BAND_MAX
        {
            return None;
        }
        if let Some((shape, index, live)) = self.slot.get() {
            // The honest +4 tag contract also rejects other layouts and
            // forwarded cells. This is one immutable shape compare and load.
            let object = addr as *const ObjectHeader;
            if unsafe { crate::object::shapes::object_shape_stamp(object) } == shape {
                return Some((object, index, live));
            }
        }
        self.locate_miss(receiver)
    }

    #[inline(never)]
    fn locate_miss(&self, receiver: f64) -> Option<(*const ObjectHeader, u32, u32)> {
        let (object, shape) = unsafe { private_plain_receiver_shape(receiver) }?;
        if unsafe { crate::object::key_attrs::object_summary(object) }
            & crate::object::key_attrs::SUMMARY_PRIVATE == 0
        {
            return None;
        }
        if unsafe { crate::object::dictionary::is_dictionary(object) } {
            let keys = unsafe { crate::object::object_keys(object) };
            let index = unsafe { crate::object::keys_find_private_slot_by_bytes(keys.arr(), keys.count(), self.as_bytes()) }?;
            return Some((object, index, unsafe { crate::object::object_live_slot_count(object) }));
        }
        let slot = self.slot.get().filter(|slot| slot.0 == shape).or_else(|| {
            if !crate::object::shapes::shape_object_kind_by_id(shape)
                .is_some_and(|kind| kind.is_ordinary_layout())
            {
                return None;
            }
            let keys = unsafe { crate::object::object_keys(object) };
            if keys.is_null() {
                return None;
            }
            let index = unsafe {
                crate::object::keys_find_private_slot_by_bytes(keys.arr(), keys.count(), self.as_bytes())
            }?;
            let attrs = unsafe { crate::object::key_attrs::keys_entry(keys.arr(), index) };
            if attrs != crate::object::key_attrs::PRIVATE_FIELD_ENTRY {
                return None;
            }
            let live = crate::object::shapes::shape_live_inline_slot_count_by_id(shape)?;
            let slot = (shape, index, live);
            self.slot.set(Some(slot));
            Some(slot)
        })?;
        Some((object, slot.1, slot.2))
    }

    #[inline]
    fn get_cached(&self, receiver: f64) -> Option<f64> {
        let (object, index, live) = self.locate(receiver)?;
        Some(f64::from_bits(unsafe {
            super::object_field_at_with_live(object, index, live).bits()
        }))
    }

    /// The field's value. A dictionary holder lists the field in its private
    /// list, which the generic own read answers; any other miss is an absent
    /// field, which reads `undefined` (the guard throws before a read of one).
    fn get(&self, receiver: f64) -> f64 {
        if let Some(value) = self.get_cached(receiver) {
            return value;
        }
        if !self.is_present(receiver) {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        crate::object::js_object_get_own_field_or_undef(receiver, self.as_ptr(), self.len())
    }

    /// Store into the private field's slot. A private field is not a
    /// property: freezing, sealing or preventing extensions of its holder
    /// does not make it read-only, so only the layout is consulted.
    fn set_cached(&self, receiver: f64, value: f64) -> bool {
        let Some((object, index, live)) = self.locate(receiver) else {
            return false;
        };
        unsafe {
            if (*object).class_id == NATIVE_MODULE_CLASS_ID {
                return false;
            }
            let object = object as *mut ObjectHeader;
            let bits = if value.to_bits() == crate::value::POINTER_TAG {
                crate::value::TAG_UNDEFINED
            } else {
                value.to_bits()
            };
            if index < live {
                crate::object::store_object_field_slot(object, index as usize, bits);
            } else if index < live.max(crate::object::INLINE_SLOT_FLOOR as u32) {
                crate::object::set_object_live_slot_count(object, index + 1);
                crate::object::store_object_field_slot(object, index as usize, bits);
            } else {
                overflow_set(object as usize, index as usize, bits);
            }
        }
        true
    }

    /// Is this field present on `receiver`'s holder? An own entry flagged
    /// `ENTRY_PRIVATE` is the field: it is never deleted.
    fn is_present(&self, receiver: f64) -> bool {
        if self.locate(receiver).is_some() {
            return true;
        }
        // A dictionary holder lists its entries in its private list.
        let Some(holder) = (unsafe { private_element_holder(receiver) }) else {
            return false;
        };
        unsafe { crate::object::key_attrs::object_key_has_private_entry(holder, self.as_bytes()) }
    }
}

struct PrivateStorageEntry {
    class_id: u32,
    evaluation_id: u64,
    name: &'static str,
    key: std::rc::Rc<PrivateStorageKey>,
}

const PRIVATE_STORAGE_CACHE_SIZE: usize = 256;
crate::perry_thread_local! {
    static PRIVATE_STORAGE_CACHE: std::cell::RefCell<[Option<PrivateStorageEntry>; PRIVATE_STORAGE_CACHE_SIZE]> =
        const { std::cell::RefCell::new([const { None }; PRIVATE_STORAGE_CACHE_SIZE]) };
}

/// The qualified name of private field `name` of `class_id` in the evaluation
/// the access resolves to (`private_storage_evaluation_id`).
fn private_storage_key(
    class_id: u32,
    receiver: Option<f64>,
    owner: Option<u64>,
    name: &str,
) -> std::rc::Rc<PrivateStorageKey> {
    // Stabilize the spelling before resolving the namespace, which may
    // allocate metadata and evacuate a caller's heap string.
    let name = intern_private_name(name.as_bytes()).unwrap_or("");
    let evaluation_id = private_storage_evaluation_id(class_id, receiver, owner);
    private_storage_key_by_id(class_id, evaluation_id, name)
}

fn private_storage_key_by_id(
    class_id: u32,
    evaluation_id: u64,
    name: &'static str,
) -> std::rc::Rc<PrivateStorageKey> {
    let hash = u64::from(class_id).wrapping_mul(0x9E37_79B9)
        ^ evaluation_id.wrapping_mul(0xC2B2_AE3D)
        ^ (name.as_ptr() as u64).wrapping_mul(0x85EB_CA77);
    let slot = (hash.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 56) as usize;
    PRIVATE_STORAGE_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(entry) = &cache[slot] {
            if entry.class_id == class_id
                && entry.evaluation_id == evaluation_id
                && entry.name == name
            {
                return entry.key.clone();
            }
        }
        let spelling = private_storage_spelling(class_id, evaluation_id, name);
        let key = std::rc::Rc::new(PrivateStorageKey {
            spelling,
            slot: std::cell::Cell::new(None),
        });
        cache[slot] = Some(PrivateStorageEntry {
            class_id,
            evaluation_id,
            name,
            key: key.clone(),
        });
        key
    })
}

fn private_storage_spelling(class_id: u32, evaluation_id: u64, name: &str) -> String {
    if evaluation_id == PRIVATE_TEMPLATE_EVALUATION_ID {
        format!("#<perry:private-value:{class_id}:{name}>")
    } else {
        format!("#<perry:private-value:{class_id}:@{evaluation_id}:{name}>")
    }
}

#[cfg(test)]
mod private_storage_cache_tests {
    use super::*;

    #[test]
    fn public_spelling_never_aliases_an_intrinsic_private_entry() {
        let scope = crate::gc::RuntimeHandleScope::new();
        let object = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
        let storage = private_storage_key_by_id(0, 0, "[[RegExpMatcher]]");
        let site = IntrinsicPrivateReadSite::new("[[RegExpMatcher]]");
        let spelling = scope.root_string_ptr(crate::string::intern_ascii_literal(storage.as_bytes()));
        object.with_mut_ptr(|obj: *mut ObjectHeader| unsafe {
            crate::object::key_attrs::apply_edits(obj, &[crate::object::key_attrs::AttrsEdit::Private(storage.as_bytes())]);
        });
        let receiver = || object.with_mut_ptr(|obj: *mut ObjectHeader| crate::value::js_nanbox_pointer(obj as i64));
        assert!(storage.set_cached(receiver(), 11.0));
        assert_eq!(site.read(receiver()), Some(11.0));
        assert_eq!(site.read(receiver()), Some(11.0));
        object.with_mut_ptr(|obj: *mut ObjectHeader| spelling.with_const_ptr(|key| {
            assert!(crate::object::js_object_get_field_by_name(obj, key).is_undefined());
            js_object_set_field_by_name(obj, key, 7.0);
        }));
        assert_eq!(storage.get_cached(receiver()), Some(11.0));
        assert_eq!(site.read(receiver()), Some(11.0));
        object.with_mut_ptr(|obj: *mut ObjectHeader| spelling.with_const_ptr(|key| {
            assert_eq!(crate::object::js_object_get_field_by_name(obj, key).as_number(), 7.0);
        }));
        assert!(storage.set_cached(receiver(), 22.0));
        assert_eq!(storage.get_cached(receiver()), Some(22.0));
        assert_eq!(site.read(receiver()), Some(22.0));
        object.with_mut_ptr::<ObjectHeader, _>(|obj| unsafe {
            assert!(crate::object::dictionary::latch_object_to_dictionary(obj));
        });
        assert_eq!(site.read(receiver()), Some(22.0));
        assert!(storage.set_cached(receiver(), 33.0));
        assert_eq!(site.read(receiver()), Some(33.0));
    }

    #[test]
    fn intrinsic_identity_zero_is_disjoint_from_class_templates_and_evaluations() {
        let template = private_storage_key_by_id(0, PRIVATE_TEMPLATE_EVALUATION_ID, "[[RegExpMatcher]]");
        let intrinsic = private_storage_key_by_id(0, 0, "[[RegExpMatcher]]");
        let evaluated = private_storage_key_by_id(0, 1, "[[RegExpMatcher]]");
        assert_ne!(template.spelling, intrinsic.spelling);
        assert_ne!(evaluated.spelling, intrinsic.spelling);
        assert_eq!(private_storage_evaluation_id(0, None, None), PRIVATE_TEMPLATE_EVALUATION_ID);
    }

    /// A field added past twenty public keys spills to overflow storage; both
    /// storage kinds read and write through the cached slot, and no property
    /// edit of the holder (a define, `freeze`) changes the field.
    #[test]
    fn template_slots_write_inline_and_overflow_and_ignore_property_edits() {
        const CID: u32 = 62_642;
        assert!(!private_template_may_be_evaluated(CID));
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(CID, 0));
        for i in 0..20 {
            let name = format!("filler{i}");
            let key = crate::string::intern_ascii_literal(name.as_bytes());
            obj.with_mut_ptr(|obj: *mut ObjectHeader| js_object_set_field_by_name(obj, key, 0.0));
            if i != 0 && i != 19 {
                continue;
            }
            let field = if i == 0 { "#inline" } else { "#spill" };
            let field_key = crate::string::intern_ascii_literal(field.as_bytes());
            obj.with_mut_ptr(|obj: *mut ObjectHeader| {
                js_private_field_add(
                    crate::value::js_nanbox_pointer(obj as i64),
                    CID,
                    crate::value::js_nanbox_string(field_key as i64),
                    11.0,
                )
            });
            let storage =
                private_storage_key_by_id(CID, PRIVATE_TEMPLATE_EVALUATION_ID, intern_private_name(field.as_bytes()).unwrap());
            let key =
                scope.root_string_ptr(crate::string::intern_ascii_literal(storage.as_bytes()));
            let read = || {
                obj.with_mut_ptr(|obj: *mut ObjectHeader| {
                    key.with_const_ptr(|key| private_evaluation_field_get(obj, key))
                })
            };
            let write = |value| {
                obj.with_mut_ptr(|obj: *mut ObjectHeader| {
                    key.with_const_ptr(|key| private_evaluation_field_set(obj, key, value))
                })
            };
            assert_eq!(read(), Some(11.0));
            let (_, index, live) = storage.slot.get().unwrap();
            assert_eq!(
                index < live,
                i == 0,
                "fixture must exercise both storage kinds"
            );
            assert!(write(37.0));
            assert_eq!(read(), Some(37.0));
            obj.with_mut_ptr(|obj: *mut ObjectHeader| unsafe {
                crate::object::key_attrs::apply_edits(
                    obj,
                    &[
                        crate::object::key_attrs::AttrsEdit::Data(storage.as_bytes(), 0),
                        crate::object::key_attrs::AttrsEdit::Accessor(
                            storage.as_bytes(),
                            true,
                            true,
                        ),
                        crate::object::key_attrs::AttrsEdit::Integrity { freeze: true },
                    ],
                )
            });
            assert!(write(99.0), "a property edit must not reach a private field");
            assert_eq!(read(), Some(99.0));
        }
    }

    #[test]
    fn qualified_keys_reuse_slots_without_retaining_receivers() {
        let scope = crate::gc::RuntimeHandleScope::new();
        let class = scope.root_raw_mut_ptr(crate::object::js_object_alloc(62_641, 0));
        class.with_mut_ptr(|class: *mut ObjectHeader| {
            crate::object::class_registry::js_object_mark_class(class as i64)
        });
        let owner = class
            .with_mut_ptr(|class: *mut ObjectHeader| crate::value::js_nanbox_pointer(class as i64));
        let key = private_instance_value_name(62_641, "#cached", owner, None);
        let again = private_instance_value_name(62_641, "#cached", owner, None);
        assert!(
            std::rc::Rc::ptr_eq(&key, &again),
            "a hot namespace must not format a new key"
        );
        let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(62_641, 0));
        let receiver = || obj.with_mut_ptr(|obj: *mut ObjectHeader| {
            crate::value::js_nanbox_pointer(obj as i64)
        });
        assert_eq!(key.get_cached(receiver()), None);
        assert!(
            key.slot.get().is_none(),
            "an uninitialized field cannot prime a slot"
        );
        // A property that merely spells the qualified name is not the field.
        let string = crate::string::intern_ascii_literal(key.as_bytes());
        obj.with_mut_ptr(|obj: *mut ObjectHeader| js_object_set_field_by_name(obj, string, 5.0));
        assert_eq!(key.get_cached(receiver()), None);
        assert!(!key.is_present(receiver()));

        let other = scope.root_raw_mut_ptr(crate::object::js_object_alloc(62_641, 0));
        let other_receiver = || other.with_mut_ptr(|obj: *mut ObjectHeader| {
            crate::value::js_nanbox_pointer(obj as i64)
        });
        let held = scope.root_raw_mut_ptr(crate::object::js_object_alloc(62_641, 0));
        let _ = held;
        // `apply_edits` suppresses moving collection for its whole body.
        other.with_mut_ptr(|holder: *mut ObjectHeader| unsafe {
            crate::object::key_attrs::apply_edits(
                holder,
                &[crate::object::key_attrs::AttrsEdit::Private(key.as_bytes())],
            );
        });
        assert!(key.set_cached(other_receiver(), 11.0));
        assert_eq!(key.get_cached(other_receiver()), Some(11.0));
        let first_shape = key.slot.get().unwrap().0;
        assert!(key.set_cached(other_receiver(), 22.0));
        assert_eq!(
            key.get_cached(other_receiver()),
            Some(22.0),
            "a cached slot must read the current value"
        );
        let extra = crate::string::intern_ascii_literal(b"extra");
        other.with_mut_ptr(|obj: *mut ObjectHeader| js_object_set_field_by_name(obj, extra, 1.0));
        assert_eq!(key.get_cached(other_receiver()), Some(22.0));
        assert_ne!(
            key.slot.get().unwrap().0,
            first_shape,
            "shape changes must re-prime"
        );

        // Churn more evaluations than the cache can hold. The scalar namespace
        // must remain distinct, and an evicted key held by a caller stays valid.
        let mut previous = String::new();
        for _ in 0..PRIVATE_STORAGE_CACHE_SIZE + 1 {
            let other = scope.root_raw_mut_ptr(crate::object::js_object_alloc(62_641, 0));
            other.with_mut_ptr(|other: *mut ObjectHeader| {
                crate::object::class_registry::js_object_mark_class(other as i64)
            });
            let owner = other.with_mut_ptr(|other: *mut ObjectHeader| {
                crate::value::js_nanbox_pointer(other as i64)
            });
            let other_key = private_instance_value_name(62_641, "#cached", owner, None);
            assert_ne!(&key.spelling, &other_key.spelling);
            assert_ne!(previous, other_key.spelling);
            previous = other_key.spelling.clone();
        }
        PRIVATE_STORAGE_CACHE.with(|cache| {
            assert!(cache.borrow().iter().flatten().count() <= PRIVATE_STORAGE_CACHE_SIZE);
        });
    }
}


/// A builtin's fixed intrinsic private read. The hit is the existing runtime
/// read site's packed shape/slot word; namespace/ENTRY_PRIVATE validation and
/// Rust key spelling live only on the miss. No receiver or value is retained.
pub(crate) struct IntrinsicPrivateReadSite {
    name: &'static str,
    site: super::runtime_read_site::RuntimeReadSite,
}

impl IntrinsicPrivateReadSite {
    pub(crate) const fn new(name: &'static str) -> Self {
        Self { name, site: super::runtime_read_site::RuntimeReadSite::new() }
    }

    /// The same evaluation-qualified spelling used by class private slots.
    /// Birth builders attach PRIVATE_FIELD_ENTRY to this canonical atom.
    #[cfg(feature = "regex-engine")]
    pub(crate) fn birth_key(&self) -> *const crate::StringHeader {
        crate::string::intern_ascii_literal(private_storage_spelling(0, 0, self.name).as_bytes())
    }

    /// Publish the same own-inline read word from a namespace-aware birth.
    ///
    /// # Safety
    /// The caller has established that this shape owns this site's qualified
    /// private key at `slot`, flagged PRIVATE_FIELD_ENTRY, with `live` inline
    /// slots. No object or key address is retained by the site.
    #[cfg(feature = "regex-engine")]
    pub(crate) unsafe fn prime_birth(&self, shape: u32, slot: u32, live: u32) {
        self.site.prime_own_inline(shape, slot, live);
    }

    #[inline]
    pub(crate) fn read(&self, receiver: f64) -> Option<f64> {
        let bits = receiver.to_bits();
        let addr = (bits & crate::value::POINTER_MASK) as usize;
        if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG
            || addr < crate::value::addr_class::HANDLE_BAND_MAX
        {
            return None;
        }
        let object = addr as *const ObjectHeader;
        if let Some(value) = unsafe { self.site.read_own_inline(object) } {
            return Some(value);
        }
        // Honest tags reject unshaped cells without private-name lookup.
        if !crate::object::shapes::is_shape_id(unsafe { (*object).parent_class_id }) {
            return None;
        }
        self.read_miss(receiver)
    }

    #[cold]
    #[inline(never)]
    fn read_miss(&self, receiver: f64) -> Option<f64> {
        let (object, _) = unsafe { private_plain_receiver_shape(receiver) }?;
        if unsafe { crate::object::key_attrs::object_summary(object) }
            & crate::object::key_attrs::SUMMARY_PRIVATE == 0
        {
            return None;
        }
        // This site already owns the shape/slot memo. A second private-name
        // cache would duplicate it and activate the class namespace cache for
        // every intrinsic-only program.
        let spelling = private_storage_spelling(0, 0, self.name);
        let keys = unsafe { crate::object::object_keys(object) };
        let index = unsafe {
            crate::object::keys_find_private_slot_by_bytes(keys.arr(), keys.count(), spelling.as_bytes())
        }?;
        let live = unsafe { crate::object::object_live_slot_count(object) };
        let shape = unsafe { crate::object::shapes::object_shape_stamp(object) };
        self.site.prime_own_inline(shape, index, live);
        Some(f64::from_bits(unsafe {
            super::object_field_at_with_live(object, index, live).bits()
        }))
    }
}

/// A runtime intrinsic's field uses the same qualified namespace as a class
/// field. Only this runtime API can request evaluation 0.
#[cfg(feature = "regex-engine")]
pub(crate) fn intrinsic_private_set(receiver: f64, name: &'static str, value: f64) -> bool {
    let key = private_storage_key_by_id(0, 0, name);
    if key.set_cached(receiver, value) {
        return true;
    }
    if !key.is_present(receiver) {
        return false;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_raw_mut_ptr(
        crate::value::js_nanbox_get_pointer(receiver) as *mut ObjectHeader,
    );
    let value = scope.root_nanbox_f64(value);
    let atom = crate::string::intern_ascii_literal(key.as_bytes());
    receiver.with_mut_ptr::<ObjectHeader, _>(|object| {
        crate::object::js_object_set_field_by_name(object, atom, value.get_nanbox_f64());
    });
    true
}

/// Install the private entry before the intrinsic publishes its receiver.
/// A property with the qualified spelling is never a private field.
#[cfg(feature = "regex-engine")]
pub(crate) fn intrinsic_private_add(receiver: f64, name: &'static str, value: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_raw_mut_ptr(
        crate::value::js_nanbox_get_pointer(receiver) as *mut ObjectHeader,
    );
    let value = scope.root_nanbox_f64(value);
    let key = private_storage_key_by_id(0, 0, name);
    receiver.with_mut_ptr::<ObjectHeader, _>(|object| unsafe {
        crate::object::key_attrs::apply_edits(
            object,
            &[crate::object::key_attrs::AttrsEdit::Private(key.as_bytes())],
        );
    });
    assert!(key.set_cached(
        receiver.with_const_ptr::<ObjectHeader, _>(|o| crate::value::js_nanbox_pointer(o as i64)),
        value.get_nanbox_f64(),
    ));
}
