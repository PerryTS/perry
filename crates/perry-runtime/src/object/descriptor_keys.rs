//! Own string keys retained from an ordinary data shape, or from the
//! family's public name snapshot. One enumeration loop consumes either.
use super::{shapes, ObjectHeader, ObjectKeys};
use crate::gc::{RuntimeHandle, RuntimeHandleScope};

pub(crate) struct OwnNamesSnapshot<'scope> {
    keys: RuntimeHandle<'scope>,
    count: u32,
    shape: u32,
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
                    shape: shapes::object_shape_id(receiver),
                };
            }
        }
        let names = super::js_object_get_own_property_names(object.get_nanbox_f64());
        let names = crate::value::js_nanbox_get_pointer(names) as *mut crate::ArrayHeader;
        let keys = ObjectKeys::owned(names);
        Self {
            keys: scope.root_raw_mut_ptr(names),
            count: keys.count(),
            shape: 0,
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
        if self.shape == 0 {
            return None;
        }
        let receiver =
            crate::value::js_nanbox_get_pointer(object.get_nanbox_f64()) as *const ObjectHeader;
        if shapes::object_shape_id(receiver) != self.shape {
            return None;
        }
        Some(f64::from_bits(
            super::js_object_get_field(receiver, index).bits(),
        ))
    }

    /// A fresh reflection result has default attributes too. When the
    /// snapshot already is that list, birth it on the final layout once;
    /// otherwise the same per-key definition flow constructs the result.
    pub(crate) unsafe fn result<'result>(
        &self,
        scope: &'result RuntimeHandleScope,
    ) -> RuntimeHandle<'result> {
        if self.shape == 0 {
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
        self.shape != 0
    }
}
