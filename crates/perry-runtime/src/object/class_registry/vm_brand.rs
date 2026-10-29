//! Link a vm.Script instance to its constructor's actual prototype.

use super::*;

pub(crate) fn brand_vm_script_instance(value: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let constructor = scope.root_nanbox_f64(
        crate::object::native_module::bound_native_callable_export_value("vm", "Script"),
    );
    let _ = ordinary_function_prototype_value_for_read(constructor.get_nanbox_f64());
    crate::node_vm::install_script_prototypes(constructor.get_nanbox_f64());
    let result = value.get_nanbox_f64();
    let result_value = JSValue::from_bits(result.to_bits());
    if result_value.is_pointer() {
        let object = result_value.as_pointer::<ObjectHeader>() as *mut ObjectHeader;
        if unsafe { crate::value::addr_class::try_read_gc_header(object as usize) }
            .is_some_and(|header| crate::gc::gc_type_is_object(header.obj_type))
        {
            let proto = js_function_prototype_value_for_read(constructor.get_nanbox_f64());
            super::super::prototype_chain::object_link_class_default_prototype(
                object as usize,
                proto.to_bits(),
            );
        }
    }
    result
}
