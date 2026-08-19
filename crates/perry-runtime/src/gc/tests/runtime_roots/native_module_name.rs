//! Moving-GC regression for short native-module names used while binding an
//! exported callable (#8403).

use super::super::super::*;
use super::super::support::*;
use crate::arena::FromSpaceProtection;

#[test]
fn dynamic_native_module_get_owns_names_across_collection() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    let _protect = crate::arena::ProtectionModeGuard::set(FromSpaceProtection::PoisonOnly);
    register_runtime_handle_root_scanner_for_tests();

    let namespace = crate::object::js_create_native_module_namespace(b"net".as_ptr(), 3);
    crate::object::test_collect_native_export_after_alloc();
    let cycles_before = copying_minor_cycles();
    let callable = unsafe {
        crate::value::js_dynamic_object_get_property(
            namespace,
            b"connect".as_ptr().cast(),
            b"connect".len(),
        )
    };

    assert!(
        copying_minor_cycles() > cycles_before,
        "the test hook must collect after callable allocation"
    );
    let addr = crate::value::js_nanbox_get_pointer(callable) as usize;
    assert!(crate::closure::is_closure_ptr(addr));
    assert_eq!(
        crate::object::builtin_closure_length(addr),
        Some(3),
        "the owned `net` name must survive collection through arity lookup"
    );
    assert_eq!(
        unsafe { crate::object::bound_native_callable_module_and_method(callable) },
        Some(("net".to_string(), "connect".to_string()))
    );
}
