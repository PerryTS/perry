//! Every pointer in every saved answer must survive and be rewritten by a
//! real moving minor, including intermediate objects and accessor pairs.
use super::super::*;
use super::support::*;
use crate::object::method_site::read_holder;
use crate::object::{ObjectHeader, PicCacheSlot};

extern "C" fn getter_seven(_this: f64) -> f64 {
    7.0
}

#[test]
fn holder_memo_overflow_roots_evacuate_and_still_hit() {
    if !crate::object::method_site::run_with_fresh_worker_gate(
        "holder_memo_overflow_roots_evacuate_and_still_hit",
    ) {
        return;
    }
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = ConservativeScanDisabledGuard::new();
    gc_register_mutable_root_scanner(read_holder::scan_read_holder_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_object_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_shape_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);
    let mut slot: PicCacheSlot = std::ptr::null_mut();
    let cache = unsafe { crate::object::field_get_set::pic_slot_resolve(&mut slot) };
    let base = crate::object::shapes::SHAPE_ID_BASE;
    unsafe {
        for id in [base + 100, base + 101, base + 102] {
            if crate::object::shapes::shape_is_retired(id) {
                assert!(crate::object::shapes::test_install_external_shape_id(
                    id,
                    std::ptr::null(),
                    0,
                    0,
                ));
            }
        }
        let (a, fields) = alloc_nursery_test_object(1);
        // GC_STORE_AUDIT(POINTER_FREE): immediate Number bits.
        *fields = 17.0f64.to_bits();
        let (hop, _) = alloc_nursery_test_object(1);
        let b = crate::object::js_object_alloc(0, 2);
        crate::object::set_builtin_accessor_pair(
            b as usize,
            "answer".to_owned(),
            crate::object::accessor_pair::Accessor {
                raw_get: getter_seven as *const () as usize,
                ..Default::default()
            },
            crate::object::PropertyAttrs::new(true, false, true),
        );
        let pair = *((b as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64)
            & crate::value::POINTER_MASK;
        assert!(crate::arena::pointer_in_nursery(a as usize));
        assert!(crate::arena::pointer_in_nursery(hop as usize));
        assert!(crate::arena::pointer_in_nursery(b as usize));
        assert!(crate::arena::pointer_in_nursery(pair as usize));
        read_holder::test_publish_holder_answer(cache, base + 100, a, Some(hop), false);
        read_holder::test_publish_holder_answer(cache, base + 101, b, None, true);
        let (c, fields) = alloc_nursery_test_object(1);
        // GC_STORE_AUDIT(POINTER_FREE): immediate Number bits.
        *fields = 19.0f64.to_bits();
        read_holder::test_publish_holder_answer(cache, base + 102, c, None, false);
        let token = |id: u32| (crate::object::shapes::PIC_ID_TOKEN_BIT | u64::from(id)) as i64;
        let recv = ObjectHeader {
            class_id: 0,
            parent_class_id: base + 101,
            meta: std::ptr::null_mut(),
        };
        assert_eq!(
            read_holder::entry_answer(&*cache, token(base + 100)),
            Some(17.0f64.to_bits())
        );
        assert_eq!(
            read_holder::try_cached_class_read(&recv, &mut slot)
                .or_else(|| read_holder::try_cached_accessor(&recv, &mut slot))
                .map(|v| v.as_number()),
            Some(7.0)
        );
        let before = read_holder::read_holder_rewrites();
        let accessor_before = read_holder::read_accessor_rewrites();
        let minor = collect_minor_trace(GcTriggerKind::Direct);
        assert!(
            minor.copying_nursery.copied_objects >= 5,
            "the test must evacuate both saved holders, the hop, the pair and the primary"
        );
        assert!(
            read_holder::read_holder_rewrites() >= before + 3,
            "all three holder pointers must rewrite"
        );
        assert!(
            read_holder::read_accessor_rewrites() > accessor_before,
            "saved accessor holder must rewrite"
        );
        let entry = read_holder::test_accessor_entry(&recv, &mut slot);
        assert!(
            !entry.is_null(),
            "the shared runtime guard backend still validates after evacuation"
        );
        assert_ne!(
            *entry.add(crate::codegen_abi::PIC_HOLDER_PAIR_WORD - read_holder::HOLDER_RECV) as u64,
            pair,
            "the saved accessor pair must rewrite, not merely its holder"
        );
        assert_eq!(
            read_holder::entry_answer(&*cache, token(base + 100)),
            Some(17.0f64.to_bits())
        );
        assert_eq!(
            read_holder::entry_answer(&*cache, token(base + 102)),
            Some(19.0f64.to_bits())
        );
        assert_eq!(
            read_holder::try_cached_class_read(&recv, &mut slot)
                .or_else(|| read_holder::try_cached_accessor(&recv, &mut slot))
                .map(|v| v.as_number()),
            Some(7.0)
        );
        // A second minor reads the already rewritten saved hop and pair.
        let _ = gc_collect_minor();
        assert_eq!(
            read_holder::entry_answer(&*cache, token(base + 100)),
            Some(17.0f64.to_bits())
        );
        assert_eq!(
            read_holder::try_cached_class_read(&recv, &mut slot)
                .or_else(|| read_holder::try_cached_accessor(&recv, &mut slot))
                .map(|v| v.as_number()),
            Some(7.0)
        );
    }
}
