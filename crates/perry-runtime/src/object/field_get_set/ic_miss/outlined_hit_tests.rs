//! Outlined reads must consume the same site facts as the inline leaf front.
use super::*;
use crate::object::method_site::read_holder::{HOLDER_KIND, HOLDER_OBJ, HOLDER_RECV, HOLDER_SHAPE};
use crate::object::shapes::{object_shape_stamp, PIC_ID_TOKEN_BIT};

#[test]
fn outlined_ways_match_inline_front_and_respect_way_state() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    let key = scope.root_string_ptr(crate::string::js_string_from_bytes(
        b"outlined_way".as_ptr(),
        12,
    ));
    let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 4));
    obj.with_mut_ptr(|o| {
        key.with_const_ptr(|k| crate::object::js_object_set_field_by_name(o, k, 17.0))
    });
    obj.with_mut_ptr::<ObjectHeader, _>(|o| unsafe {
        let token = (u64::from((*o).parent_class_id) | PIC_ID_TOKEN_BIT) as i64;
        assert_ne!(object_shape_stamp(o), 0);
        for w in 0..PIC_WAYS {
            let mut cache: PicCache = [0; PIC_CACHE_WORDS];
            cache[PIC_WAY_STATE] = 1;
            cache[PIC_WAY_BASE + 2 * w] = token;
            cache[PIC_WAY_BASE + 2 * w + 1] = 0;
            let mut slot: PicCacheSlot = &mut cache;
            assert_eq!(pic_outlined_mru_hit(o, &mut slot), Some(17.0));
            key.with_const_ptr::<crate::StringHeader, _>(|k| {
                let packed = std::sync::atomic::AtomicU64::new(PACKED_GET_EMPTY);
                assert_eq!(
                    crate::object::js_object_get_field_ic_front(
                        crate::object::shapes::ordinary_dir_addr(),
                        (o as i64).wrapping_sub(perry_abi::RECEIVER_HANDLE_FLOOR as i64),
                        crate::value::js_nanbox_string(k as i64).to_bits(),
                        &mut slot,
                        &packed,
                    ),
                    17.0
                );
            });
            for state in [0, -1] {
                cache[PIC_WAY_STATE] = state;
                assert_eq!(pic_outlined_mru_hit(o, &mut slot), None);
            }
            cache[PIC_WAY_STATE] = 1;
            cache[PIC_WAY_BASE + 2 * w] = token + 1;
            assert_ne!(cache[PIC_WAY_BASE + 2 * w], token, "premise: another shape");
            assert_eq!(pic_outlined_mru_hit(o, &mut slot), None);
        }
        let mut null_slot = std::ptr::null_mut();
        assert_eq!(pic_outlined_mru_hit(o, &mut null_slot), None);
    });
}

#[test]
fn outlined_holder_checks_receiver_and_holder_shapes_on_every_use() {
    if !crate::object::method_site::run_with_fresh_worker_gate(
        "outlined_holder_checks_receiver_and_holder_shapes_on_every_use",
    ) {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    let key = scope.root_string_ptr(crate::string::js_string_from_bytes(
        b"outlined_holder".as_ptr(),
        15,
    ));
    let recv = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 4));
    let holder = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 4));
    holder.with_mut_ptr(|h| {
        key.with_const_ptr(|k| crate::object::js_object_set_field_by_name(h, k, 23.0))
    });
    recv.with_mut_ptr::<ObjectHeader, _>(|r| {
        holder.with_mut_ptr::<ObjectHeader, _>(|h| unsafe {
            crate::object::js_object_set_prototype_of(
                crate::value::js_nanbox_pointer(r as i64),
                crate::value::js_nanbox_pointer(h as i64),
            );
            assert_ne!(object_shape_stamp(r), 0);
            let mut cache: PicCache = [0; PIC_CACHE_WORDS];
            // The existing depth-1 data entry layout; no new cache or roots.
            cache[HOLDER_RECV] = (u64::from(object_shape_stamp(r)) | PIC_ID_TOKEN_BIT) as i64;
            cache[HOLDER_OBJ] = h as i64;
            cache[HOLDER_SHAPE] = i64::from(object_shape_stamp(h));
            cache[HOLDER_KIND] = 0;
            let mut slot: PicCacheSlot = &mut cache;
            assert_eq!(pic_outlined_mru_hit(r, &mut slot), Some(23.0));
            key.with_const_ptr(|k| crate::object::js_object_set_field_by_name(h, k, 29.0));
            assert_eq!(pic_outlined_mru_hit(r, &mut slot), Some(29.0));
            key.with_const_ptr(|k| crate::object::js_object_set_field_by_name(r, k, 31.0));
            assert_eq!(
                pic_outlined_mru_hit(r, &mut slot),
                None,
                "own shadow invalidates receiver"
            );
            cache[HOLDER_RECV] = (u64::from(object_shape_stamp(r)) | PIC_ID_TOKEN_BIT) as i64;
            cache[HOLDER_SHAPE] += 1;
            assert_ne!(cache[HOLDER_SHAPE], i64::from(object_shape_stamp(h)));
            assert_eq!(
                pic_outlined_mru_hit(r, &mut slot),
                None,
                "stale holder invalidates entry"
            );
            // Small native handles and Proxy IDs must never be dereferenced.
            for handle in [0, 0x40000, 0xF0000] {
                assert_eq!(
                    pic_outlined_mru_hit(handle as *const ObjectHeader, &mut slot),
                    None
                );
            }
            let array = crate::array::js_array_alloc(1);
            assert_eq!(
                pic_outlined_mru_hit(array as *const ObjectHeader, &mut slot),
                None
            );
        });
    });
}
