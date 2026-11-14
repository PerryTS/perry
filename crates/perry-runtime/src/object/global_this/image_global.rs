//! Values whose source birth is owned by a native namespace stay on that
//! birth arm until native image membership is available. Only selected Global
//! slots are wired; the rest of the realm is already emitted data.
use super::*;

pub(super) unsafe fn birth_providers(global: *mut ObjectHeader) {
    let record = crate::object::shapes::object_shape_record(global)
        .expect("image Global has its published row");
    let keys = record.keys_view();
    for index in 0..record.logical_key_count() {
        let entry = crate::object::key_attrs::keys_entry(keys.arr(), index);
        if crate::object::key_attrs::entry_is_private(entry) || entry & 8 != 0 {
            continue;
        }
        let Some(slot) = crate::object::key_attrs::list_storage_slot(keys.arr(), index) else {
            panic!("image Global has nonordinal storage");
        };
        let fields = crate::meta_slots_of!(crate::meta_obj_runtime_born!(global));
        if *fields.add(slot as usize) != crate::value::TAG_UNDEFINED {
            continue;
        }
        let key = crate::array::array_elements_ptr(keys.arr())
            .cast::<u64>()
            .add(index as usize)
            .read();
        let value = crate::value::JSValue::from_bits(key);
        if !value.is_string() {
            continue;
        }
        let key = value.as_string_ptr();
        let bytes = crate::string::string_as_str(key).as_bytes();
        let name = std::str::from_utf8(bytes).expect("Global key is UTF-8");
        let value = if let Some(value) = super::populate::native_constructor_value(name) {
            value
        } else if let Some(value) = super::populate::performance_constructor_value(name) {
            if name == "Performance" {
                super::populate::link_performance_intrinsics();
            }
            value
        } else if name == "performance" {
            crate::perf_hook_slots::performance_namespace()
        } else if GLOBAL_THIS_BUILTIN_NAMESPACES.contains(&name) {
            js_create_native_module_namespace(bytes.as_ptr(), bytes.len())
        } else {
            continue;
        };
        // GC_STORE_AUDIT(BARRIERED): the source provider writes a selected,
        // existing Global slot after its row and roots have been published.
        let address = fields.add(slot as usize);
        address.write(value.to_bits());
        crate::gc::runtime_write_barrier_slot(global as usize, address as usize, value.to_bits());
        crate::object::builtin_namespace::prepare_selected_binding(name, value.to_bits());
    }
}
