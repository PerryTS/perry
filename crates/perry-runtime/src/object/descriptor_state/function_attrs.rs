//! Function descriptor writes use the ordinary own-property bag, including
//! accessor pairs. Keep the function's shape in sync with its bag's keys.
use super::ObjectHeader;

pub(crate) struct FunctionBagEdit {
    owner: usize,
    pub(super) bag: *mut ObjectHeader,
    _no_move: crate::gc::GcSuppressScope,
}

impl FunctionBagEdit {
    #[inline]
    pub(crate) fn new(owner: usize) -> Option<Self> {
        if !crate::closure::is_closure_ptr(owner) {
            return None;
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

impl Drop for FunctionBagEdit {
    fn drop(&mut self) {
        crate::closure::shape::refresh_closure_shape(self.owner);
    }
}

#[cfg(test)]
#[path = "function_attrs_tests.rs"]
mod tests;
