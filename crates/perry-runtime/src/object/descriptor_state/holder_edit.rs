//! Normalize descriptor writes to each owner's ordinary property holder.
//! A function additionally refreshes its receiver shape after editing its bag.
use super::ObjectHeader;

pub(crate) struct HolderEdit {
    owner: usize,
    pub(super) bag: *mut ObjectHeader,
    _no_move: crate::gc::GcSuppressScope,
}

impl HolderEdit {
    #[inline]
    pub(crate) fn new(owner: usize) -> Option<Self> {
        if !crate::closure::is_closure_ptr(owner) {
            let _no_move = crate::gc::GcSuppressScope::new();
            let bag = unsafe {
                let header = crate::value::addr_class::try_read_tracked_gc_header(owner);
                match header.map(|h| (*h.as_ptr()).obj_type) {
                    Some(crate::gc::GC_TYPE_ARRAY) => crate::array::array_property_bag_ensure(
                        owner as *mut crate::array::ArrayHeader,
                    ),
                    Some(crate::gc::GC_TYPE_LAZY_ARRAY) => {
                        let array = crate::json_tape::force_materialize_lazy(
                            owner as *mut crate::json_tape::LazyArrayHeader,
                        );
                        crate::array::array_property_bag_ensure(array)
                    }
                    Some(crate::gc::GC_TYPE_OBJECT) => {
                        let live = super::filter::resolve_object_holder(owner);
                        if live == owner {
                            return None;
                        }
                        // An evacuation alias is not a native registry handle.
                        // Normalize writes as well as reads to the live holder.
                        return Some(Self {
                            owner: live,
                            bag: live as *mut ObjectHeader,
                            _no_move,
                        });
                    }
                    _ if crate::buffer::header::is_owned_byte_cell(owner) => {
                        crate::buffer::store::bag_ensure(owner)
                    }
                    Some(
                        crate::gc::GC_TYPE_ERROR
                        | crate::gc::GC_TYPE_MAP
                        | crate::gc::GC_TYPE_SET
                        | crate::gc::GC_TYPE_PROMISE
                        | crate::gc::GC_TYPE_DATE_CELL,
                    ) => super::super::cell_expando_ensure(owner)?,
                    Some(crate::gc::GC_TYPE_TEMPORAL) => {
                        super::super::exotic_expando::property_bag_ensure(owner)
                    }
                    None => super::super::handle_expando::handle_property_bag_ensure(owner as i64),
                    Some(kind) => {
                        debug_assert!(false, "GC cell type {kind} is not a descriptor holder");
                        return None;
                    }
                }
            };
            return Some(Self {
                owner,
                bag,
                _no_move,
            });
        }
        Some(Self::for_closure(owner))
    }

    #[inline(never)]
    fn for_closure(owner: usize) -> Self {
        let no_move = crate::gc::GcSuppressScope::new();
        super::super::prop_plan::prop_plan_epoch_bump_for_owner(owner);
        // SAFETY: a proven live closure, held in a no-move window.
        let bag = unsafe { crate::closure::props::bag_ensure(owner) };
        Self {
            owner,
            bag,
            _no_move: no_move,
        }
    }
    #[inline(never)]
    pub(super) fn materialize_data_keys(&self, edits: &[super::AttrsEdit<'_>]) {
        if !crate::closure::is_closure_ptr(self.owner) {
            return;
        }
        for edit in edits {
            let super::AttrsEdit::Data(key, _) = edit else {
                continue;
            };
            let Ok(name) = std::str::from_utf8(key) else {
                continue;
            };
            unsafe {
                if !crate::closure::props::bag_has_own(self.owner, key)
                    && super::super::has_own_helpers::closure_own_key_present(self.owner, name)
                {
                    let value = crate::closure::closure_get_dynamic_prop(self.owner, name);
                    crate::closure::props::bag_define_value(self.owner, name, value);
                }
            }
        }
    }
}

impl Drop for HolderEdit {
    fn drop(&mut self) {
        if crate::closure::is_closure_ptr(self.owner) {
            crate::closure::shape::refresh_closure_shape(self.owner);
        }
    }
}

#[cfg(test)]
#[path = "function_attrs_tests.rs"]
mod tests;
