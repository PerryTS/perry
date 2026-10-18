//! Sendable function-local counter state, with module authorities detached.
use super::*;

pub(crate) struct OwnedCounter {
    value: Cell<u32>,
    byte_owner_root_slots: RefCell<Vec<String>>,
    eh_scopes: RefCell<Vec<EhScope>>,
    shadow_slot_allocas: RefCell<HashSet<String>>,
    null_guard_symbol: RefCell<Option<String>>,
    stable_packed_revalidation_slots: RefCell<Vec<String>>,
    byte_access_dirty_slots: RefCell<Vec<String>>,
    guard_memo_slots: RefCell<Vec<String>>,
    gc_effects: RefCell<Option<Arc<crate::gc_call_effects::GcEffects>>>,
}
impl RegCounter {
    pub(crate) fn into_owned(self) -> OwnedCounter {
        let Self {
            value,
            byte_owner_root_slots,
            eh_scopes,
            shadow_slot_allocas,
            preserve_none_fns,
            fn_infos,
            null_guard_symbol,
            stable_packed_revalidation_slots,
            byte_access_dirty_slots,
            guard_memo_slots,
            gc_effects,
        } = self;
        drop((preserve_none_fns, fn_infos));
        OwnedCounter {
            value,
            byte_owner_root_slots,
            eh_scopes,
            shadow_slot_allocas,
            null_guard_symbol,
            stable_packed_revalidation_slots,
            byte_access_dirty_slots,
            guard_memo_slots,
            gc_effects,
        }
    }
}
impl OwnedCounter {
    pub(crate) fn into_builder(self) -> RegCounter {
        let Self {
            value,
            byte_owner_root_slots,
            eh_scopes,
            shadow_slot_allocas,
            null_guard_symbol,
            stable_packed_revalidation_slots,
            byte_access_dirty_slots,
            guard_memo_slots,
            gc_effects,
        } = self;
        RegCounter {
            value,
            byte_owner_root_slots,
            eh_scopes,
            shadow_slot_allocas,
            null_guard_symbol,
            stable_packed_revalidation_slots,
            byte_access_dirty_slots,
            guard_memo_slots,
            gc_effects,
            preserve_none_fns: RefCell::new(None),
            fn_infos: RefCell::new(None),
        }
    }
}
