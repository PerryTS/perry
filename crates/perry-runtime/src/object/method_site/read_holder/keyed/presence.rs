//! HasProperty consumes the same keyed entries, without reading a value lane.
use super::*;

pub(crate) unsafe fn presence_at_site(obj: f64, key: f64, slot: *mut *mut KeyedCache) -> f64 {
    if slot.is_null() {
        return crate::object::js_in_operator(obj, key);
    }
    let enabled = WORKER_AGENTS_EXIST.load(Ordering::SeqCst) == 0
        && crate::agent::current_agent() == crate::agent::PRIMARY_AGENT;
    if !enabled {
        return crate::object::js_in_operator(obj, key);
    }
    if enabled {
        if let Some(recv) = receiver(obj.to_bits()) {
            let cache = crate::object::pic_slot_peek(slot);
            if !cache.is_null() {
                let token = (u64::from((*recv).parent_class_id) | PIC_ID_TOKEN_BIT) as i64;
                for i in 0..(*cache).entries.len() {
                    let entry = &(*cache).entries[i];
                    if entry.identity_key == key.to_bits()
                        && entry.token == token
                        && entry.valid_shapes()
                    {
                        // First way is the literal site's own-positive proof.
                        // Other ways retain all existing keyed answers. No
                        // new word, flag or invalidation state is introduced.
                        let present = !entry.absent;
                        if i != 0 && entry.depth == 0 && present {
                            (*cache).entries.swap(0, i);
                        }
                        return f64::from_bits(crate::JSValue::bool(present).bits());
                    }
                }
                // As in computed reads, keep hits in the frozen ways but
                // stop deriving proofs that the publisher cannot install.
                if (*cache).evictions >= MAX_EVICTIONS {
                    return crate::object::js_in_operator(obj, key);
                }
            }
        }
    }
    presence_miss(obj, key, slot)
}

// One cold body for this shared operation. Cached answers do not carry its
// proof/hop/publication frame at every call site or every runtime hit.
#[cold]
#[inline(never)]
unsafe fn presence_miss(obj: f64, key: f64, slot: *mut *mut KeyedCache) -> f64 {
    if let Some(p) =
        crate::object::field_get_set::has_property::shape_presence::presence_proof(obj, key)
    {
        if p.reusable {
            let recv = receiver(obj.to_bits()).unwrap();
            let cache = crate::object::pic_slot_resolve(slot);
            let mut hops = NO_HOPS;
            hops[..p.hops.len()].copy_from_slice(&p.hops);
            let walk = Walk {
                holder: p.holder,
                holder_shape: p.holder_shape,
                slot: p.slot,
                hops,
                depth: p.depth,
                getter: 0,
            };
            // Keep inherited/absent validation in the shared operation.
            // Publishing through ways 1..3 leaves way 0 an own-positive
            // shape fact; an own proof promotes there, preserving its old
            // proof in the publishing way.
            if let Some(i) = publish_keyed(&mut *cache, recv, key.to_bits(), &walk, 1) {
                if p.depth == 0 && p.present {
                    (*cache).entries.swap(0, i);
                }
            }
        }
        return f64::from_bits(crate::JSValue::bool(p.present).bits());
    }
    // Coercion, traps and exotic receivers keep the complete operation.
    crate::object::field_get_set::has_property::in_operator_generic(obj, key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presence_entries_reject_mutated_hops_and_never_read_values() {
        if !super::super::super::super::run_with_fresh_worker_gate(
            "presence_entries_reject_mutated_hops_and_never_read_values",
        ) {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_gc = crate::gc::GcSuppressScope::new();
        // Memo logic with young objects: nothing moves (above), and the
        // publication rule is waived for them.
        let _admit = crate::gc::never_move::test_admit::Guard::new();
        crate::object::ensure_object_intrinsics();
        unsafe {
            let key = crate::string::canonical_key(b"presence_entry");
            let boxed_key = crate::value::js_nanbox_string(key as i64);
            let proto = crate::object::object_alloc_plain(1);
            let middle = crate::object::object_alloc_plain(0);
            let recv = crate::object::object_alloc_plain(0);
            let boxed = crate::value::js_nanbox_pointer(recv as i64);
            crate::object::js_object_set_prototype_of(
                crate::value::js_nanbox_pointer(middle as i64),
                crate::value::js_nanbox_pointer(proto as i64),
            );
            crate::object::js_object_set_prototype_of(
                boxed,
                crate::value::js_nanbox_pointer(middle as i64),
            );
            let mut slot = std::ptr::null_mut();
            assert_eq!(
                presence_at_site(boxed, boxed_key, &mut slot).to_bits(),
                crate::value::TAG_FALSE
            );
            assert!(!slot.is_null(), "subject: absent proof published");
            assert_eq!(
                (*slot).entries[0].token,
                0,
                "absence cannot arm the own-positive inline way"
            );
            let stale = (*slot)
                .entries
                .iter()
                .find(|e| e.token != 0)
                .copied()
                .unwrap();
            assert!(stale.valid_shapes());
            crate::object::js_object_set_field_by_name(
                middle,
                key,
                f64::from_bits(crate::value::TAG_UNDEFINED),
            );
            assert!(
                !stale.valid_shapes(),
                "an intermediate key addition revokes absence"
            );
            assert_eq!(
                presence_at_site(boxed, boxed_key, &mut slot).to_bits(),
                crate::value::TAG_TRUE
            );
            assert_eq!(
                (*slot).entries[0].token,
                0,
                "inherited presence keeps holder validation out of line"
            );
            let positive = (*slot)
                .entries
                .iter()
                .find(|e| !e.absent && e.token != 0)
                .copied()
                .unwrap();
            assert!(
                positive.valid_shapes(),
                "subject: inherited proof published"
            );
            assert!((*slot).registered, "the shared root registry owns the site");
            let mut marked = Vec::new();
            super::scan_site_keys(
                slot as usize,
                &mut crate::gc::RuntimeRootVisitor::for_copy(&mut |v| {
                    marked.push(v.to_bits() & crate::value::POINTER_MASK)
                }),
            );
            // The key is a root (compared by identity); the holder is a
            // stable weak word and is not (`read_holder.rs`, "Weak entries").
            assert!(
                !marked.contains(&(middle as u64)),
                "the inherited holder is not traced"
            );
            assert!(
                marked.contains(&(key as u64)),
                "the keyed identity is traced"
            );

            // Value is undefined throughout; membership, not a value probe.
            crate::object::js_object_delete_field(middle, key);
            assert!(
                !positive.valid_shapes(),
                "deleting the holder key revokes presence"
            );
            assert_eq!(
                presence_at_site(boxed, boxed_key, &mut slot).to_bits(),
                crate::value::TAG_FALSE
            );
            crate::object::js_object_set_field_by_name(
                recv,
                key,
                f64::from_bits(crate::value::TAG_UNDEFINED),
            );
            assert_eq!(
                presence_at_site(boxed, boxed_key, &mut slot).to_bits(),
                crate::value::TAG_TRUE
            );
            let own = (*slot).entries[0];
            assert_eq!(own.token as u32, object_shape_stamp(recv));
            assert_eq!(own.depth, 0);
            assert!(!own.absent, "only own-positive proofs reach the inline way");
            assert!(crate::object::dictionary::latch_object_to_dictionary(recv));
            crate::object::js_object_delete_field(recv, key);
            assert_eq!(
                presence_at_site(boxed, boxed_key, &mut slot).to_bits(),
                crate::value::TAG_FALSE
            );
            crate::object::js_object_set_field_by_name(
                recv,
                key,
                f64::from_bits(crate::value::TAG_UNDEFINED),
            );
            assert_eq!(
                presence_at_site(boxed, boxed_key, &mut slot).to_bits(),
                crate::value::TAG_TRUE
            );
            assert!(
                !(*slot)
                    .entries
                    .iter()
                    .any(|e| e.token as u32 == object_shape_stamp(recv)),
                "dictionary shapes cannot enter the memo"
            );
            // Saturation retains existing hits, but misses answer directly
            // without replacing a frozen proof or growing publication state.
            let frozen = crate::object::object_alloc_plain(1);
            crate::object::js_object_set_field_by_name(
                frozen,
                key,
                f64::from_bits(crate::value::TAG_UNDEFINED),
            );
            let frozen_value = crate::value::js_nanbox_pointer(frozen as i64);
            assert_eq!(
                presence_at_site(frozen_value, boxed_key, &mut slot).to_bits(),
                crate::value::TAG_TRUE
            );
            (*slot).evictions = MAX_EVICTIONS;
            let frozen_token = (*slot).entries[0].token;
            assert_eq!(
                presence_at_site(frozen_value, boxed_key, &mut slot).to_bits(),
                crate::value::TAG_TRUE
            );
            let fresh = crate::object::object_alloc_plain(2);
            crate::object::js_object_set_field_by_name(
                fresh,
                crate::string::canonical_key(b"frozen_extra"),
                1.0,
            );
            crate::object::js_object_set_field_by_name(
                fresh,
                key,
                f64::from_bits(crate::value::TAG_UNDEFINED),
            );
            assert_ne!(object_shape_stamp(fresh), frozen_token as u32);
            let fresh_value = crate::value::js_nanbox_pointer(fresh as i64);
            assert_eq!(
                presence_at_site(fresh_value, boxed_key, &mut slot).to_bits(),
                crate::value::TAG_TRUE
            );
            crate::object::js_object_delete_field(fresh, key);
            assert_eq!(
                presence_at_site(fresh_value, boxed_key, &mut slot).to_bits(),
                crate::value::TAG_FALSE
            );
            assert_eq!((*slot).entries[0].token, frozen_token);
            assert_eq!((*slot).evictions, MAX_EVICTIONS);
        }
    }
}
