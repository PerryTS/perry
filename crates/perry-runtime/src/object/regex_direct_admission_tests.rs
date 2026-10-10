use crate::object::regex_proto_thunks::{method_site_test_code, regex_proto_test_direct};
use crate::value::js_nanbox_get_pointer;

#[test]
fn a_constfn_exec_lane_must_name_the_builtin_body() {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(|| unsafe {
            let _stable = crate::gc::GcSuppressScope::new();
            let re = crate::regex::test_construct_regexp_and_exec_once("a", "");
            let proto = js_nanbox_get_pointer(crate::regex::instance::intrinsic_prototype())
                as *mut crate::object::ObjectHeader;
            let method_info = |name| {
                let key = crate::string::js_string_from_str(name);
                let value = crate::object::js_object_get_field_by_name_f64(proto, key);
                let closure = js_nanbox_get_pointer(value) as *const crate::closure::ClosureHeader;
                &*(*closure).info
            };
            let mut holder = super::object_shape_descriptor(proto).expect("prototype shape");
            let test = method_info("test");
            let word = (re as *const u64).read();
            let direct = regex_proto_test_direct as *const () as u64;
            assert_eq!(method_site_test_code(test, word, &holder, 1), direct);
            let exec = crate::object::keys_find_slot_by_bytes_resolved(
                holder.keys as usize as *const crate::array::ArrayHeader,
                holder.logical_key_count,
                b"exec",
            )
            .expect("exec slot");
            let mut infos = holder.constfn_infos().to_vec();
            infos
                .iter_mut()
                .find(|entry| u32::from(entry.slot) == exec)
                .unwrap()
                .info = method_info("toString") as *const crate::closure::JsFunctionInfo as u64;
            // Retain the ConstFn fact but name another body. Replacing exec on
            // the real prototype revokes the lane and tests an earlier guard.
            let extras = super::shapes_store::ShapeExtras {
                constfn_infos: infos.into_boxed_slice(),
                brands: Box::default(),
                to_nopointer: std::sync::atomic::AtomicU32::new(0),
                to_any: std::sync::atomic::AtomicU32::new(0),
                rollback_parent: std::sync::atomic::AtomicU32::new(0),
                created_birth_shape: std::sync::atomic::AtomicU32::new(0),
            };
            holder.record = 0;
            holder.extras = &extras as *const super::shapes_store::ShapeExtras as u64;
            assert_ne!(method_site_test_code(test, word, &holder, 1), direct);
        })
        .unwrap()
        .join()
        .unwrap();
}
