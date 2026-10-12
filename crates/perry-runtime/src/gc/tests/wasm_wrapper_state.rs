//! Each trusted binding follows its wrapper, never copied public properties.
use crate::gc::tests::support::*;
use crate::gc::*;
use crate::object::test_wasm_wrapper_state::{attach, handle, Kind};
#[test]
fn module_state_follows_moving_wrapper_and_is_collectible() {
    check(Kind::Module, false);
}
#[test]
fn extern_state_follows_moving_wrapper_and_is_collectible() {
    for kind in [Kind::Memory, Kind::Table, Kind::Global] {
        check(kind, false);
    }
}
#[test]
#[ignore = "negative control: remove the module wrapper's native state edge"]
fn negative_control_objtables_wasm_module_wrappers() {
    check(Kind::Module, true);
}
#[test]
#[ignore = "negative control: remove the external wrapper's native state edge"]
fn negative_control_objtables_wasm_extern_wrappers() {
    check(Kind::Memory, true);
}
fn check(kind: Kind, erase: bool) {
    let _guard = CopyingNurseryTestGuard::new(2);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = ConservativeScanDisabledGuard::new();
    crate::weakref::test_support::clear_weak_holders();
    let wrapper = crate::object::js_object_alloc(0, 0);
    attach(wrapper as usize, kind, 0x1234);
    let cell = unsafe { (*(*wrapper).meta).native_state };
    let weak = crate::weakref::js_weakref_new(f64::from_bits(cell));
    js_shadow_slot_set(0, crate::value::js_nanbox_pointer(wrapper as i64).to_bits());
    js_shadow_slot_set(1, crate::value::js_nanbox_pointer(weak as i64).to_bits());
    let _ = gc_collect_minor();
    let moved = (js_shadow_slot_get(0) & crate::value::POINTER_MASK) as *mut crate::ObjectHeader;
    assert_ne!(moved, wrapper);
    let fresh = crate::object::js_object_alloc(0, 0);
    assert_eq!(
        handle(fresh as usize, kind),
        None,
        "public lookalike fields cannot brand a new owner"
    );
    assert_eq!(
        handle(
            moved as usize,
            if kind == Kind::Module {
                Kind::Memory
            } else {
                Kind::Module
            }
        ),
        None
    );
    if erase {
        unsafe {
            (*(*moved).meta).native_state = 0;
        }
    }
    js_gc_collect();
    assert_ne!(
        crate::weakref::js_weakref_deref(f64::from_bits(js_shadow_slot_get(1))).to_bits(),
        crate::value::TAG_UNDEFINED,
        "only the wrapper's meta-to-native-state edge retains the binding"
    );
    assert_eq!(handle(moved as usize, kind), Some(0x1234));
    js_shadow_slot_set(0, 0);
    js_gc_collect();
    assert_eq!(
        crate::weakref::js_weakref_deref(f64::from_bits(js_shadow_slot_get(1))).to_bits(),
        crate::value::TAG_UNDEFINED
    );
}
