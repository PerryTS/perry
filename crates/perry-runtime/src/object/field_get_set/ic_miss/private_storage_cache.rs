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
    fn locate(&self, receiver: f64) -> Option<(*const ObjectHeader, u32, u32)> {
        let (object, shape) = unsafe { private_plain_receiver_shape(receiver) }?;
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
                crate::object::keys_find_slot_by_bytes(keys.arr(), keys.count(), self.as_bytes())
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
            if (*object).class_id == NATIVE_MODULE_CLASS_ID
                || crate::object::dictionary::is_dictionary(object)
            {
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
        unsafe { crate::object::key_attrs::object_key_is_private(holder, self.as_bytes()) }
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
        let spelling = if evaluation_id == 0 {
            format!("#<perry:private-value:{class_id}:{name}>")
        } else {
            format!("#<perry:private-value:{class_id}:@{evaluation_id}:{name}>")
        };
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

#[cfg(test)]
mod private_storage_cache_tests {
    use super::*;

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
                private_storage_key_by_id(CID, 0, intern_private_name(field.as_bytes()).unwrap());
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
