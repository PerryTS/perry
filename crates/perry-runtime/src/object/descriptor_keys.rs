//! Own string keys retained from an ordinary data shape, or from the
//! family's public name snapshot. One enumeration loop consumes either.
use super::{shapes, ObjectHeader, ObjectKeys};
use crate::codegen_abi::LayoutWord;
use crate::gc::{RuntimeHandle, RuntimeHandleScope};

/// Property facts owned by descriptor resolution. Exotic families keep their
/// public descriptor representation; ordinary keys carry values and flags.
pub(crate) enum OwnProperty {
    Data(f64, super::PropertyAttrs),
    Accessor(f64, f64, super::PropertyAttrs),
    Public(f64),
}

impl OwnProperty {
    pub(crate) fn data(
        value: f64,
        writable: bool,
        enumerable: bool,
        configurable: bool,
    ) -> Option<Self> {
        Some(Self::Data(
            value,
            super::PropertyAttrs::new(writable, enumerable, configurable),
        ))
    }

    pub(crate) fn accessor(
        get: f64,
        set: f64,
        enumerable: bool,
        configurable: bool,
    ) -> Option<Self> {
        Some(Self::Accessor(
            get,
            set,
            super::PropertyAttrs::new(false, enumerable, configurable),
        ))
    }

    pub(crate) fn public(descriptor: f64) -> Option<Self> {
        (descriptor.to_bits() != crate::value::TAG_UNDEFINED).then_some(Self::Public(descriptor))
    }

    pub(crate) unsafe fn materialize(self) -> f64 {
        match self {
            Self::Data(value, attrs) => super::build_data_descriptor(
                value,
                attrs.writable(),
                attrs.enumerable(),
                attrs.configurable(),
            ),
            Self::Accessor(get, set, attrs) => {
                super::build_accessor_descriptor(get, set, attrs.enumerable(), attrs.configurable())
            }
            Self::Public(descriptor) => descriptor,
        }
    }

    /// A clone reads a data value or invokes the getter itself. It never
    /// materializes ordinary descriptors, and retains exotic public behavior.
    #[optimize(size)]
    pub(crate) unsafe fn value_and_flags(self) -> (u64, u8) {
        use super::key_attrs::{attr_bits_to_entry, ENTRY_ACCESSOR, ENTRY_NON_ENUMERABLE};
        match self {
            Self::Data(value, attrs) => (value.to_bits(), attr_bits_to_entry(attrs.bits)),
            Self::Accessor(get, _, attrs) => (
                get.to_bits(),
                ENTRY_ACCESSOR | attr_bits_to_entry(attrs.bits),
            ),
            Self::Public(descriptor) => {
                let obj = crate::value::js_nanbox_get_pointer(descriptor) as *const ObjectHeader;
                let keys = super::object_keys(obj);
                let field = |name: &[u8]| {
                    super::keys_find_property_slot_by_bytes(keys, keys.count(), name)
                        .map(|slot| super::field_get_set::js_object_get_key_field(obj, slot).bits())
                };
                let flags = if field(b"enumerable") == Some(crate::value::TAG_TRUE) {
                    0
                } else {
                    ENTRY_NON_ENUMERABLE
                };
                match field(b"value") {
                    Some(value) => (value, flags),
                    None => (
                        field(b"get").unwrap_or(crate::value::TAG_UNDEFINED),
                        flags | ENTRY_ACCESSOR,
                    ),
                }
            }
        }
    }
}

pub(crate) struct OwnNamesSnapshot<'scope> {
    keys: RuntimeHandle<'scope>,
    count: u32,
    shape: LayoutWord,
}

/// A plain ordinary data-key list answers own presence, attributes and slot
/// position together. No exotic surface or hidden/private key is admitted.
pub(crate) unsafe fn ordinary_data_shape(
    value: f64,
) -> Option<(*const ObjectHeader, shapes::ShapeDescriptor)> {
    let value = crate::JSValue::from_bits(value.to_bits());
    if !value.is_pointer() {
        return None;
    }
    let object = value.as_pointer::<ObjectHeader>();
    let header = crate::value::addr_class::try_read_gc_header(object as usize)?;
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return None;
    }
    // A shadowed object's own keys are its overflow's, not the layout's.
    if super::overflow::shadowed(object) {
        return None;
    }
    let shape = shapes::object_shape_descriptor(object)?;
    (matches!(
        shape.object_kind,
        shapes::ShapeObjectKind::Ordinary | shapes::ShapeObjectKind::OrdinaryUnmarked
    ) && shape.summary == 0
        && shape.hole_count == 0)
        .then_some((object, shape))
}

impl<'scope> OwnNamesSnapshot<'scope> {
    pub(crate) unsafe fn collect(
        scope: &'scope RuntimeHandleScope,
        object: &RuntimeHandle<'_>,
    ) -> Self {
        if let Some((receiver, shape)) = ordinary_data_shape(object.get_nanbox_f64()) {
            let keys = ObjectKeys::new(
                shape.keys as *mut crate::ArrayHeader,
                shape.logical_key_count,
            );
            // Public snapshots must sort integer keys first. Reuse their
            // existing ordered builder whenever the shape needs reordering.
            if shape.object_kind == shapes::ShapeObjectKind::Ordinary
                && (keys.is_null() || shapes::keys_prefix_is_immutable(keys.arr()))
                && !super::keys_contain_array_index(keys)
            {
                return Self {
                    keys: scope.root_raw_mut_ptr(keys.arr()),
                    count: keys.count(),
                    shape: LayoutWord(shapes::object_layout_stamp(receiver).0),
                };
            }
        }
        let names = super::js_object_get_own_property_names(object.get_nanbox_f64());
        let names = crate::value::js_nanbox_get_pointer(names) as *mut crate::ArrayHeader;
        let keys = ObjectKeys::owned(names);
        Self {
            keys: scope.root_raw_mut_ptr(names),
            count: keys.count(),
            shape: LayoutWord(0),
        }
    }

    pub(crate) fn count(&self) -> u32 {
        self.count
    }

    pub(crate) unsafe fn key(&self, index: u32) -> f64 {
        self.keys.with_mut_ptr::<crate::ArrayHeader, _>(|keys| {
            f64::from_bits(ObjectKeys::new(keys, self.count).get(index).bits())
        })
    }

    /// Revalidate the snapshot's shape at every use: decoding a preceding
    /// descriptor can invoke user code that edits the source bag. A live
    /// matching ShapeId proves this key still has default own-data attrs;
    /// its value is read now, so value-only mutations remain observable.
    pub(crate) unsafe fn data_value(&self, object: &RuntimeHandle<'_>, index: u32) -> Option<f64> {
        if self.shape == LayoutWord(0) {
            return None;
        }
        let receiver =
            crate::value::js_nanbox_get_pointer(object.get_nanbox_f64()) as *const ObjectHeader;
        if shapes::object_layout_stamp(receiver) != self.shape {
            return None;
        }
        Some(f64::from_bits(
            super::js_object_get_field(receiver, index).bits(),
        ))
    }

    /// Read the retained key with its live own value/accessor and attributes.
    /// The immutable shape prefix needs no name or descriptor allocation;
    /// every use revalidates its ShapeId. Public key snapshots resolve through
    /// the same descriptor authority after mutations or for other families.
    #[optimize(size)]
    pub(crate) unsafe fn property(
        &self,
        object: &RuntimeHandle<'_>,
        index: u32,
    ) -> Option<(f64, OwnProperty)> {
        let key = self.key(index);
        let property = match self.data_value(object, index) {
            Some(value) => OwnProperty::data(value, true, true, true),
            None => super::own_property(object.get_nanbox_f64(), key),
        }?;
        // Resolution of an exotic family can allocate: reload the rooted key.
        Some((self.key(index), property))
    }

    /// A fresh reflection result has default attributes too. When the
    /// snapshot already is that list, birth it on the final layout once;
    /// otherwise the same per-key definition flow constructs the result.
    pub(crate) unsafe fn result<'result>(
        &self,
        scope: &'result RuntimeHandleScope,
    ) -> RuntimeHandle<'result> {
        if self.shape == LayoutWord(0) {
            return scope.root_raw_mut_ptr(super::js_object_alloc(0, 0));
        }
        let result =
            scope.root_raw_mut_ptr(super::alloc_basic::object_alloc_unpublished(0, self.count));
        result.with_mut_ptr::<ObjectHeader, _>(|object| {
            shapes::store_kind::premark_plain_ordinary(object);
            // The retained prefix already belongs to a published immutable
            // shape. Publication interns facts without a nursery allocation.
            self.keys.with_mut_ptr::<crate::ArrayHeader, _>(|keys| {
                super::set_object_keys_with_live(
                    object,
                    ObjectKeys::new(keys, self.count),
                    self.count,
                );
            });
        });
        result
    }

    pub(crate) fn has_final_result_keys(&self) -> bool {
        self.shape != LayoutWord(0)
    }
}
