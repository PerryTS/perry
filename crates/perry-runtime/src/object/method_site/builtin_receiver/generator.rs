//! Reuse the builtin method memo for the shared synchronous protocol holder.
//! Births retain distinct function prototypes. A hit proves that both the
//! instance and its first prototype have no own keys, and that the latter's
//! live shape links to the actual intrinsic holder. No property name is used
//! in this receiver proof; the existing memo still validates the holder slot
//! and callable identity on every use.
use super::*;

pub(super) unsafe fn shared_protocol_family(addr: usize) -> Option<u32> {
    let object = addr as *const ObjectHeader;
    let meta = (*object).meta;
    if meta.is_null() || !crate::native_payload::is_payload_state_word((*meta).native_state) {
        return None;
    }
    let empty = |object: *const ObjectHeader| {
        let shape = super::super::super::shapes::object_shape_record(object)?;
        let meta = (*object).meta;
        (shape.object_kind().is_ordinary_layout()
            && shape.logical_key_count() == 0
            && (*object).class_id == 0
            && (meta.is_null()
                || (*meta).flags & crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER == 0))
            .then_some(shape)
    };
    let receiver = empty(object)?;
    if !receiver.has_brand(crate::object::global_this::GENERATOR_STATE_BRAND) {
        return None;
    }
    // Both links belong to class-zero ordinary objects. Admit the existing
    // serial/default/null identity band, whose live record pins the edge.
    // Class, mixed-class and per-object identities retain ordinary lookup.
    if receiver.proto_id() >= super::super::super::shapes::PROTO_ID_CLASS {
        return None;
    }
    let first = read_holder::next_from_word(object, receiver.prototype_word());
    let intrinsic = crate::object::GENERATOR_PROTOTYPE_PTR.load(Ordering::Acquire);
    if intrinsic == 0 {
        return None;
    }
    // GetPrototypeFromConstructor also admits the realm holder directly when
    // the source function's prototype is primitive. The born shape owns that
    // exact edge; no intermediate empty prototype is needed on this chain.
    if first as i64 == intrinsic {
        return Some(0);
    }
    let header = crate::value::addr_class::try_read_gc_header(first as usize)?;
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return None;
    }
    let prototype = empty(first)?;
    if prototype.proto_id() >= super::super::super::shapes::PROTO_ID_CLASS {
        return None;
    }
    if read_holder::next_from_word(first, prototype.prototype_word()) as i64 != intrinsic {
        return None;
    }
    // The complete live empty-chain proof has the same property answer for
    // every function. Its identity serial need not occupy another memo entry.
    Some(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn owner(_: *const crate::closure::ClosureHeader, _: crate::closure::JsThis) -> f64 {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    }
    extern "C" fn resume(
        _: *const crate::closure::ClosureHeader,
        _: crate::closure::JsThis,
        _: f64,
        _: f64,
    ) -> f64 {
        42.0
    }

    #[test]
    fn generator_protocol_memo_unifies_functions_and_rejects_overrides() {
        if !run_with_fresh_worker_gate(
            "generator_protocol_memo_unifies_functions_and_rejects_overrides",
        ) {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let _stable = crate::gc::GcSuppressScope::new();
            // Memo logic with young per-instance hops: nothing moves (above),
            // and the publication rule is waived for them.
            let _admit = crate::gc::never_move::test_admit::Guard::new();
            let object_was_lazy = crate::object::OBJECT_INTRINSIC_PTR.load(Ordering::Acquire) == 0;
            let parent_was_lazy =
                crate::object::OBJECT_INTRINSIC_PROTO_PTR.load(Ordering::Acquire) == 0;
            let mut slot = std::ptr::null_mut();
            let mut shapes = std::collections::HashSet::new();
            let mut last = 0.0;
            for _ in 0..8 {
                let owner = crate::closure::js_closure_alloc(
                    crate::fn_info!(owner, 0; with_flags(crate::closure::FN_GENERATOR)),
                    0,
                );
                let state = crate::closure::js_closure_alloc(crate::fn_info!(resume, 2), 4);
                let recv = crate::object::js_generator_create(
                    crate::value::js_nanbox_pointer(state as i64),
                    owner,
                );
                let object = crate::value::js_nanbox_get_pointer(recv) as *const ObjectHeader;
                shapes.insert(super::super::super::super::shapes::object_shape_stamp(
                    object,
                ));
                super::super::prime(&mut slot, recv, b"next");
                assert!(super::super::lookup_hit(&mut slot, recv.to_bits()).is_some());
                assert!(!site_is_megamorphic(&mut slot));
                last = recv;
            }
            let site = crate::object::pic_slot_peek(&mut slot);
            assert!(
                (*site)
                    .entries
                    .iter()
                    .any(|entry| entry.word != u64::MAX && entry.word as u32 == 0),
                "subject: recurring shapes have an ordinary chain proof without Object bootstrap"
            );
            assert_eq!(
                shapes.len(),
                8,
                "subject: eight distinct born receiver shapes"
            );
            if object_was_lazy {
                assert_eq!(
                    crate::object::OBJECT_INTRINSIC_PTR.load(Ordering::Acquire),
                    0,
                    "generator births need the parent shape, not Object materialization"
                );
            }
            if parent_was_lazy {
                assert_eq!(
                    crate::object::OBJECT_INTRINSIC_PROTO_PTR.load(Ordering::Acquire),
                    0,
                    "the live protocol holder proof does not need an Object parent"
                );
            }
            let site = crate::object::pic_slot_peek(&mut slot);
            assert_eq!(
                (*site)
                    .entries
                    .iter()
                    .filter(|entry| entry.word != u64::MAX && entry.word as u32 != 0)
                    .count(),
                1
            );
            assert_eq!(
                super::super::call_hit(&mut slot, last, std::ptr::null(), 0),
                Some(42.0),
                "a live family proof enters the private resume ABI"
            );
            let fallback_owner = crate::closure::js_closure_alloc(
                crate::fn_info!(owner, 0; with_flags(crate::closure::FN_GENERATOR)),
                0,
            );
            crate::closure::closure_define_dynamic_prop(fallback_owner as usize, "prototype", 42.0);
            let fallback_state = crate::closure::js_closure_alloc(crate::fn_info!(resume, 2), 4);
            let fallback = crate::object::js_generator_create(
                crate::value::js_nanbox_pointer(fallback_state as i64),
                fallback_owner,
            );
            assert!(
                super::super::lookup_hit(&mut slot, fallback.to_bits()).is_some(),
                "a primitive source prototype uses the same intrinsic holder proof"
            );
            let (selected, _, _, _) = super::super::lookup_hit(&mut slot, last.to_bits()).unwrap();
            let fake = crate::object::js_object_alloc(0, 0);
            let fake = crate::value::js_nanbox_pointer(fake as i64);
            assert!(
                super::super::call_selected(
                    &mut slot,
                    f64::from_bits(selected),
                    fake,
                    std::ptr::null(),
                    0
                )
                .is_none(),
                "selected callable identity alone does not prove the receiver brand"
            );
            let first = crate::object::js_object_get_prototype_of(last);
            let first = crate::value::js_nanbox_get_pointer(first) as *mut ObjectHeader;
            let next = crate::string::canonical_key(b"next");
            crate::object::js_object_set_field_by_name(first, next, 901.0);
            assert!(
                super::super::lookup_hit(&mut slot, last.to_bits()).is_none(),
                "a first-prototype override revokes the family proof"
            );
            // Worker activation disables memo reads, not protocol semantics.
            WORKER_AGENTS_EXIST.store(1, Ordering::SeqCst);
            crate::object::js_object_set_field_by_name(first, next, 0.0);
            let failure = crate::exception::catch_js_throw(|| {
                super::super::call_miss(&mut slot, last, next as i64, std::ptr::null(), 0)
            });
            assert!(
                failure.is_err(),
                "non-callable next throws with the memo disabled"
            );
            let fake = crate::object::js_object_alloc(0, 0);
            assert!(
                shared_protocol_family(fake as usize).is_none(),
                "brand is required"
            );
        }
    }
}
