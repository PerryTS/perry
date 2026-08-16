//! Brand a `vm.Script` instance with the synthetic class id of the bound
//! `vm.Script` constructor so `instanceof` / prototype walks match Node.

use super::*;

pub(crate) fn brand_vm_script_instance(value: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let constructor = scope.root_nanbox_f64(
        crate::object::native_module::bound_native_callable_export_value("vm", "Script"),
    );
    let class_id = synthetic_class_id_for_function(constructor.get_nanbox_f64());
    if class_id == 0 {
        return value.get_nanbox_f64();
    }
    let _ = ordinary_function_prototype_value_for_read(constructor.get_nanbox_f64());
    crate::node_vm::install_script_prototypes(constructor.get_nanbox_f64());
    let result = value.get_nanbox_f64();
    let result_value = JSValue::from_bits(result.to_bits());
    if result_value.is_pointer() {
        let object = result_value.as_pointer::<ObjectHeader>() as *mut ObjectHeader;
        if unsafe { crate::value::addr_class::try_read_gc_header(object as usize) }
            .is_some_and(|header| header.obj_type == crate::gc::GC_TYPE_OBJECT)
        {
            unsafe { (*object).class_id = class_id };
        }
    }
    result
}
