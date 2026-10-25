//! `delete` on an ordinary object (object model step 4, §3.2/§3.4): a delete
//! makes the object's overflow its order authority. The layout word keeps
//! its layout id (the shadow bit marks the overflow), the deleted key's
//! fixed slot holds the hole marker, and a re-add appends to the overflow as
//! a reference to that same fixed slot. These replace the tombstone-lane and
//! delete-transition pins (#9029, #9064, #10768): deletes no longer reach
//! that lane, so its scenarios are asserted here against the overflow.

use super::super::{js_object_alloc, js_object_get_field_by_name, js_object_set_field_by_name};

/// Restores the per-thread tombstone-flag override on scope exit (panic
/// included) so a failing test cannot leak flag-on deletes into unrelated
/// tests on the same thread.
fn scopeguard_tombstone_flag() -> impl Drop {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::object::delete_rest::test_set_tombstone_deletes(None);
        }
    }
    Restore
}

fn key(name: &str) -> *const crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

/// Twenty inline keys `{prefix}00..{prefix}19` holding their index.
unsafe fn receiver_20(class_id: u32, prefix: &str) -> *mut crate::object::ObjectHeader {
    let obj = js_object_alloc(class_id, 24);
    for i in 0..20 {
        js_object_set_field_by_name(obj, key(&format!("{prefix}{i:02}")), i as f64);
    }
    obj
}

fn layout_of(obj: *const crate::object::ObjectHeader) -> u32 {
    crate::object::layout_desc::layout_id_of_word(unsafe { super::shapes::object_shape_stamp(obj) })
}

unsafe fn own_names(obj: *mut crate::object::ObjectHeader) -> Vec<String> {
    let keys = crate::object::js_object_keys(obj);
    (0..crate::array::js_array_length(keys))
        .map(|i| {
            let key = crate::array::js_array_get(keys, i);
            let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            String::from_utf8(
                crate::string::js_string_key_bytes(key, &mut scratch)
                    .unwrap()
                    .to_vec(),
            )
            .unwrap()
        })
        .collect()
}

/// The delete establishes the overflow, keeps the layout id, holes the fixed
/// slot, and every reader skips the deleted key: the JSON prefix template
/// (#9029's SEGV) and structured clone included.
#[test]
fn delete_establishes_the_overflow_and_keeps_the_layout() {
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let obj = receiver_20(0, "key_number_");
        let key_count = super::shapes::object_shape_descriptor(obj)
            .unwrap()
            .logical_key_count;
        assert!(!crate::object::overflow::has_overflow(obj));
        assert_eq!(
            super::delete_rest::js_object_delete_field(obj, key("key_number_03")),
            1
        );
        assert!(crate::object::overflow::has_overflow(obj));
        assert_eq!(
            super::shapes::object_shape_descriptor(obj)
                .unwrap()
                .logical_key_count,
            key_count,
            "a delete keeps the layout's keys (its slot mapping)"
        );
        assert_ne!(
            (*obj).parent_class_id & crate::object::overflow::SHADOW_BIT,
            0,
            "the shadow bit marks the overflow"
        );
        let fields =
            (obj as *mut u8).add(std::mem::size_of::<crate::object::ObjectHeader>()) as *const u64;
        assert_eq!(
            *fields.add(3),
            crate::value::TAG_HOLE,
            "the deleted key's fixed slot holds the hole marker"
        );
        assert!(js_object_get_field_by_name(obj, key("key_number_03")).is_undefined());
        for i in [0usize, 2, 4, 19] {
            assert_eq!(
                js_object_get_field_by_name(obj, key(&format!("key_number_{i:02}"))).bits(),
                (i as f64).to_bits(),
                "a delete must not move a surviving key"
            );
        }
        assert_eq!(own_names(obj).len(), 19);
        assert!(!own_names(obj).iter().any(|n| n == "key_number_03"));

        let bits = crate::value::POINTER_TAG | (obj as u64 & crate::value::POINTER_MASK);
        if let Some(t) = crate::json::stringify_shape_template::build_shape_prefix_template(bits) {
            assert!(
                !t.prefixes.iter().any(|p| p.contains("key_number_03")),
                "the template must not resurrect a deleted key"
            );
        }
        let payload = crate::child_process::v8_serde::v8_serialize(f64::from_bits(bits));
        let back = crate::child_process::v8_serde::v8_deserialize(&payload);
        let back_obj =
            crate::value::js_nanbox_get_pointer(back) as *mut crate::object::ObjectHeader;
        assert_eq!(
            own_names(back_obj).len(),
            19,
            "structured clone must not serialize the deleted key"
        );
    }
}

/// A re-add appends (enumeration order moves it to the end) and meets its
/// fixed slot again; delete/re-add churn keeps the overflow bounded.
#[test]
fn readd_appends_in_order_meets_its_slot_and_churn_stays_bounded() {
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let obj = receiver_20(0, "hc_key_");
        for victim in ["hc_key_11", "hc_key_03"] {
            assert_eq!(
                super::delete_rest::js_object_delete_field(obj, key(victim)),
                1
            );
        }
        js_object_set_field_by_name(obj, key("hc_key_03"), 99.0);
        let names = own_names(obj);
        assert_eq!(names.last().map(String::as_str), Some("hc_key_03"));
        assert_eq!(names.len(), 19);
        let fields =
            (obj as *mut u8).add(std::mem::size_of::<crate::object::ObjectHeader>()) as *const u64;
        assert_eq!(
            *fields.add(3),
            99.0f64.to_bits(),
            "the re-add meets its fixed slot"
        );
        for round in 0..60 {
            assert_eq!(
                super::delete_rest::js_object_delete_field(obj, key("hc_key_05")),
                1
            );
            js_object_set_field_by_name(obj, key("hc_key_05"), f64::from(round));
        }
        assert_eq!(
            js_object_get_field_by_name(obj, key("hc_key_05")).bits(),
            59.0f64.to_bits()
        );
        let t = crate::object::overflow::table(obj);
        assert!(
            (*t).length <= 40,
            "churned 20-key object holds {} overflow entries: compaction never ran",
            (*t).length
        );
    }
}

/// Delete/re-add churn does not mint per delete (the tombstone lane minted
/// one ShapeId per delete); a delete only generalizes the holed slot's lane
/// once.
#[test]
fn delete_readd_churn_does_not_mint_per_delete() {
    let _global = crate::gc::global_side_table_test_lock();
    const CYCLES: u32 = 200;
    unsafe {
        let obj = receiver_20(0, "idcount_");
        assert_eq!(
            super::delete_rest::js_object_delete_field(obj, key("idcount_19")),
            1
        );
        let before = super::shapes::test_shape_id_counter();
        for round in 0..CYCLES {
            let name = format!("idcount_{:02}", round % 10);
            assert_eq!(
                super::delete_rest::js_object_delete_field(obj, key(&name)),
                1
            );
            js_object_set_field_by_name(obj, key(&name), f64::from(round));
        }
        let minted = super::shapes::test_shape_id_counter() - before;
        assert!(
            minted < CYCLES,
            "{minted} ShapeIds over {CYCLES} delete/re-add cycles"
        );
    }
}

/// Two receivers deleting the same keys, in either order and from the same
/// layout, agree on every answer and on enumeration order: each delete holes
/// its own fixed slot, nothing shifts.
#[test]
fn deletes_in_any_order_agree_on_every_answer() {
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let forward = receiver_20(0, "order_key_");
        let backward = receiver_20(0, "order_key_");
        assert_eq!(layout_of(backward), layout_of(forward));
        for (obj, order) in [(forward, [3usize, 9]), (backward, [9usize, 3])] {
            for slot in order {
                let name = format!("order_key_{slot:02}");
                assert_eq!(
                    super::delete_rest::js_object_delete_field(obj, key(&name)),
                    1
                );
            }
            assert!(crate::object::overflow::has_overflow(obj));
        }
        assert_eq!(own_names(forward), own_names(backward));
        for obj in [forward, backward] {
            for slot in [3usize, 9] {
                let name = format!("order_key_{slot:02}");
                assert!(js_object_get_field_by_name(obj, key(&name)).is_undefined());
            }
            assert_eq!(
                js_object_get_field_by_name(obj, key("order_key_10")).bits(),
                10.0f64.to_bits()
            );
        }
    }
}

/// Class instances, registered anonymous-shape literals and objects used as
/// prototypes delete through the overflow too; the delete is visible through
/// the prototype chain.
#[test]
fn every_ordinary_receiver_kind_deletes_through_the_overflow() {
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        const ANON_ID: u32 = 0x6E06_4001;
        crate::object::js_register_anon_shape_class_id(ANON_ID);
        for (class_id, prefix) in [(0x0004_2101, "instT_"), (ANON_ID, "anonT_")] {
            let obj = receiver_20(class_id, prefix);
            for victim in [19u32, 6] {
                let name = format!("{prefix}{victim:02}");
                assert_eq!(
                    super::delete_rest::js_object_delete_field(obj, key(&name)),
                    1
                );
                assert!(js_object_get_field_by_name(obj, key(&name)).is_undefined());
            }
            assert!(crate::object::overflow::has_overflow(obj));
            assert_eq!(
                js_object_get_field_by_name(obj, key(&format!("{prefix}07"))).bits(),
                7.0f64.to_bits()
            );
        }
        let proto = receiver_20(0, "protoT_");
        let child = js_object_alloc(0, 0);
        crate::object::js_object_set_prototype_of(
            crate::value::js_nanbox_pointer(child as i64),
            crate::value::js_nanbox_pointer(proto as i64),
        );
        assert_eq!(
            js_object_get_field_by_name(child, key("protoT_06")).bits(),
            6.0f64.to_bits()
        );
        assert_eq!(
            super::delete_rest::js_object_delete_field(proto, key("protoT_06")),
            1
        );
        assert!(
            js_object_get_field_by_name(child, key("protoT_06")).is_undefined(),
            "the delete must be visible through the prototype chain"
        );
    }
}

/// A descriptor installed after the overflow exists still refuses a delete
/// of its non-configurable key, and leaves the value.
#[test]
fn a_later_non_configurable_descriptor_refuses_delete() {
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let obj = receiver_20(0, "descriptor_key_");
        for victim in ["descriptor_key_11", "descriptor_key_03"] {
            assert_eq!(
                super::delete_rest::js_object_delete_field(obj, key(victim)),
                1
            );
        }
        super::descriptor_state::set_property_attrs(
            obj as usize,
            "descriptor_key_05".to_string(),
            super::descriptor_state::PropertyAttrs::new(true, true, false),
        );
        assert_eq!(
            super::delete_rest::js_object_delete_field(obj, key("descriptor_key_05")),
            0
        );
        assert_eq!(
            js_object_get_field_by_name(obj, key("descriptor_key_05")).bits(),
            5.0f64.to_bits()
        );
    }
}

/// An old receiver (born outside the nursery, so no minor enumerates it) with
/// 20 keys, two of them deleted (the delete installs a young metadata record
/// and a young overflow), then `added_after`.
unsafe fn old_receiver_with_young_overflow() -> *mut crate::object::ObjectHeader {
    let slots = 24usize;
    let header_size = std::mem::size_of::<crate::object::ObjectHeader>();
    let obj = crate::arena::arena_alloc_gc_old(
        header_size + slots * std::mem::size_of::<u64>(),
        8,
        crate::gc::GC_TYPE_OBJECT,
    ) as *mut crate::object::ObjectHeader;
    (*obj).class_id = 0;
    (*obj).parent_class_id = 0;
    (*obj).meta = std::ptr::null_mut();
    let fields = (obj as *mut u8).add(header_size) as *mut u64;
    for i in 0..slots {
        // GC_STORE_AUDIT(INIT): fresh unpublished storage, pointer-free.
        std::ptr::write(fields.add(i), crate::value::TAG_UNDEFINED);
    }
    crate::gc::layout_init_pointer_free(obj as *mut u8);
    super::shapes::birth_publish_object_shape(obj, slots as u32);
    assert!(!crate::arena::pointer_in_nursery(obj as usize));
    for i in 0..20 {
        js_object_set_field_by_name(obj, key(&format!("key_number_{i:02}")), i as f64);
    }
    for victim in ["key_number_11", "key_number_03"] {
        assert_eq!(
            super::delete_rest::js_object_delete_field(obj, key(victim)),
            1
        );
    }
    js_object_set_field_by_name(obj, key("added_after"), 42.0);
    obj
}

/// Every key and value of [`old_receiver_with_young_overflow`] reads back.
unsafe fn assert_old_receiver_intact(obj: *mut crate::object::ObjectHeader, at: &str) {
    assert!(crate::object::overflow::has_overflow(obj), "{at}");
    assert_eq!(own_names(obj).len(), 19, "{at}");
    for i in 0..20u32 {
        let value = js_object_get_field_by_name(obj, key(&format!("key_number_{i:02}")));
        if i == 3 || i == 11 {
            assert!(value.is_undefined(), "{at}");
        } else {
            assert_eq!(
                value.bits(),
                f64::from(i).to_bits(),
                "key {i} after the minor, {at}"
            );
        }
    }
    assert_eq!(
        js_object_get_field_by_name(obj, key("added_after")).bits(),
        42.0f64.to_bits(),
        "{at}"
    );
}

/// A receiver born outside the nursery (no minor enumerates it) keeps its
/// young overflow, keys and values across a minor: the overflow edge from
/// its old metadata is remembered (#9200's failure shape).
#[test]
fn an_old_receiver_keeps_its_overflow_across_a_minor() {
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let obj = old_receiver_with_young_overflow();
        let _ = crate::gc::gc_collect_minor();
        assert_old_receiver_intact(obj, "one receiver");
    }
}

/// Flag-off compaction mutates an owned keys array in place. Its post-delete
/// count can equal a historical prefix count from growth, but the shifted key
/// order makes that old ShapeId semantically stale (#9064 differential pin).
#[test]
fn tombstone_off_compaction_does_not_reuse_growth_prefix_shape() {
    super::delete_rest::test_set_tombstone_deletes(Some(false));
    let _restore = scopeguard_tombstone_flag();
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let obj = js_object_alloc(0, 0);
        for i in 0..99 {
            let name = format!("compact_key_{i:03}");
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            js_object_set_field_by_name(obj, key, i as f64);
        }
        let growth_prefix = super::shapes::object_shape_stamp(obj);
        let last = crate::string::js_string_from_bytes(b"compact_key_099".as_ptr(), 15);
        js_object_set_field_by_name(obj, last, 99.0);

        let victim = crate::string::js_string_from_bytes(b"compact_key_050".as_ptr(), 15)
            as *const crate::StringHeader;
        assert_eq!(super::delete_rest::js_object_delete_field(obj, victim), 1);
        assert_ne!(
            super::shapes::object_shape_stamp(obj),
            growth_prefix,
            "in-place compaction reused a growth-era prefix ShapeId"
        );
        let shifted = crate::string::js_string_from_bytes(b"compact_key_098".as_ptr(), 15);
        assert_eq!(
            js_object_get_field_by_name(obj, shifted).bits(),
            98.0f64.to_bits()
        );
    }
}

/// A receiver carrying a DESCRIPTOR is outside the stable-tombstone lane
/// (`OBJ_FLAG_HAS_DESCRIPTORS` re-opens the full semantic checks). Its delete
/// must still transition, and a non-configurable key must still refuse.
#[test]
fn delete_with_descriptors_transitions_and_still_refuses_non_configurable() {
    super::delete_rest::test_set_tombstone_deletes(Some(true));
    let _restore = scopeguard_tombstone_flag();
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let obj = js_object_alloc(0, 0);
        for i in 0..20 {
            let name = format!("desc_key_{i:02}");
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            js_object_set_field_by_name(obj, key, i as f64);
        }
        super::descriptor_state::set_property_attrs(
            obj as usize,
            "desc_key_05".to_string(),
            super::descriptor_state::PropertyAttrs::new(true, true, false),
        );
        let locked = crate::string::js_string_from_bytes(b"desc_key_05".as_ptr(), 11);
        let before = super::shapes::object_shape_stamp(obj);
        assert_eq!(
            super::delete_rest::js_object_delete_field(obj, locked),
            0,
            "a non-configurable key must refuse"
        );
        assert_eq!(
            super::shapes::object_shape_stamp(obj),
            before,
            "a REFUSED delete must not move the shape word"
        );
        let victim = crate::string::js_string_from_bytes(b"desc_key_11".as_ptr(), 11);
        assert_eq!(super::delete_rest::js_object_delete_field(obj, victim), 1);
        assert_ne!(
            super::shapes::object_shape_stamp(obj),
            before,
            "a descriptor-carrying receiver must transition its ShapeId on delete"
        );
        assert_eq!(
            js_object_get_field_by_name(obj, locked).bits(),
            5.0f64.to_bits()
        );
    }
}

/// The same receiver at every position on several generation pages. The
/// remembered set is page granular, so a receiver whose metadata edge is not
/// remembered still survives when a neighbour happened to dirty its page:
/// whether it did depends on everything allocated old before it (a smaller
/// static layout set was enough to move it onto a clean page). Each round
/// shifts the old bump position by a pointer-free filler, and every round
/// must remember the edge itself and keep the overflow.
#[test]
fn an_old_receiver_keeps_its_overflow_at_every_page_offset() {
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let mut pages = Vec::new();
        for round in 0..256usize {
            let _filler = crate::arena::arena_alloc_gc_old(
                8 * (1 + round % 7),
                8,
                crate::gc::GC_TYPE_STRING,
            );
            let obj = old_receiver_with_young_overflow();
            let meta_slot = std::ptr::addr_of!((*obj).meta) as usize;
            let at = format!("round {round}, meta slot {meta_slot:#x}");
            assert!(crate::arena::pointer_in_nursery((*obj).meta as usize), "premise, {at}");
            assert!(
                crate::gc::old_slot_page_is_remembered_for_test(meta_slot),
                "the young metadata edge is not remembered, {at}"
            );
            let _ = crate::gc::gc_collect_minor();
            assert_old_receiver_intact(obj, &at);
            let page = crate::arena::generation_page_for_addr(meta_slot);
            if !pages.contains(&page) {
                pages.push(page);
            }
            if pages.len() > 4 {
                return;
            }
        }
        panic!("premise: the sweep visited only {} generation pages", pages.len());
    }
}
