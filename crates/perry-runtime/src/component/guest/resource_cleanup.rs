//! Only private ownership state is held strongly. It has no edge back to an
//! owned wrapper; borrowed wrappers retain their owner through PARENT instead.
//! User destructors run as jobs, never from the collector's weak pass.
use super::*;
use crate::object::owned_state_bag::COMPONENT_FINALIZATION;

fn registry(owner: f64) -> f64 {
    unsafe {
        bag::load(
            object_pointer(owner).unwrap() as usize,
            COMPONENT_FINALIZATION,
        )
    }
}

pub(super) fn register(wrapper: f64, fields: f64) {
    let scope = RuntimeHandleScope::new();
    let wrapper = scope.root_nanbox_f64(wrapper);
    let fields = scope.root_nanbox_f64(fields);
    let owner = scope.root_nanbox_f64(get(fields.get_nanbox_f64(), OWNER));
    let registry = scope.root_nanbox_f64(registry(owner.get_nanbox_f64()));
    if JSValue::from_bits(registry.get_nanbox_f64().to_bits()).is_undefined() {
        let callback = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
            crate::fn_info!(collected, 1),
            0,
        ));
        registry.set_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::weakref::js_finreg_new(crate::value::js_nanbox_pointer(
                callback.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
            )) as i64,
        ));
        unsafe {
            bag::store(
                object_pointer(owner.get_nanbox_f64()).unwrap() as usize,
                COMPONENT_FINALIZATION,
                registry.get_nanbox_f64(),
            );
        }
    }
    crate::weakref::js_finreg_register(
        registry.get_nanbox_f64(),
        wrapper.get_nanbox_f64(),
        fields.get_nanbox_f64(),
        wrapper.get_nanbox_f64(),
    );
}

pub(super) fn unregister(wrapper: f64) {
    let fields = data(wrapper);
    let registry = registry(get(fields, OWNER));
    if !JSValue::from_bits(registry.to_bits()).is_undefined() {
        crate::weakref::js_finreg_unregister(registry, wrapper);
    }
}

extern "C" fn collected(
    _: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
    fields: f64,
) -> f64 {
    release(fields)
}
