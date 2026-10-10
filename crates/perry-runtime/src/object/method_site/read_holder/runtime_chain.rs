//! Runtime sites extend the existing holder record past its inline hop words.
//! The proof block has the chain's actual length; publication, hits, eviction
//! and moving-GC roots remain the class_read record's existing mechanism.
use super::*;

/// Called on a runtime site's miss. A deep data/absence proof is itself
/// ordinary Get; it never runs a getter. Short entries keep the shared miss
/// entry. Walks allocate Rust proof storage, never JS cells.
pub(crate) unsafe fn read_runtime_chain(
    receiver: *const ObjectHeader,
    name: &[u8],
    cache_slot: *mut PicCacheSlot,
) -> Option<f64> {
    if WORKER_AGENTS_EXIST.load(Ordering::SeqCst) != 0
        || crate::agent::current_agent() != crate::agent::PRIMARY_AGENT
        || cache_slot.is_null()
        || !read_name_admitted(receiver, name)
    {
        return None;
    }
    let Some(receiver) = ordinary_receiver(receiver as usize) else {
        return None;
    };
    if key_may_be_accessor(receiver, name) {
        return None;
    }
    let Some(shape) = object_shape_descriptor(receiver) else {
        return None;
    };
    // Alias reads can forward into native state after an ordinary miss.
    if shape.object_kind == crate::object::shapes::ShapeObjectKind::OrdinaryNativeAlias {
        return None;
    }
    // An own value or accessor cannot be replaced by a chain proof, even if
    // its current answer happens to equal an inherited value or undefined.
    let keys = shape.keys as usize as *const crate::array::ArrayHeader;
    if !keys.is_null()
        && crate::object::keys_find_slot_by_bytes_resolved(keys, shape.logical_key_count, name)
            .is_some()
    {
        return None;
    }
    let class_first = class_link(receiver).is_some();
    let bound = if class_first {
        class_read::CLASS_READ_MAX_DEPTH
    } else {
        HOLDER_MAX_DEPTH
    };
    if walk_to(receiver, name, class_first, bound).is_some() {
        return None;
    }
    let proof = walk(receiver, name)?;
    let bits = match proof.slot {
        None => crate::value::TAG_UNDEFINED,
        Some(slot) => holder_slot_value(proof.holder, slot)?,
    };
    if bits == crate::value::TAG_HOLE {
        return None;
    }
    let cache = crate::object::field_get_set::pic_slot_resolve::<PicCache>(cache_slot);
    if !cache.is_null() {
        class_read::publish_chain(
            cache,
            receiver,
            &class_read::Chain {
                holder: proof.holder,
                holder_shape: proof.holder_shape,
                slot: proof.slot,
                depth: proof.depth,
                hops: &proof.hops,
                ordinary_get: true,
            },
        );
    }
    Some(f64::from_bits(bits))
}

struct Proof {
    holder: usize,
    holder_shape: u32,
    slot: Option<u32>,
    depth: usize,
    hops: Vec<Hop>,
}

unsafe fn walk(receiver: *const ObjectHeader, name: &[u8]) -> Option<Proof> {
    let object_prototype = crate::array::object_prototype_addr_if_resolved();
    let mut current = receiver;
    let mut hops = Vec::<Hop>::new();
    loop {
        let terminal = current != receiver && current as usize == object_prototype;
        let first_class = current == receiver && class_link(receiver).is_some();
        let (pid, word) = if terminal {
            (PROTO_ID_NULL, 0)
        } else if first_class {
            (PROTO_ID_CLASS, 0)
        } else {
            admitted_link(current)?
        };
        if pid == PROTO_ID_NULL {
            let (holder, holder_shape, depth) = match hops.pop() {
                Some((holder, shape)) => (holder, shape, hops.len() + 1),
                None => (0, 0, 0),
            };
            return Some(Proof {
                holder,
                holder_shape,
                slot: None,
                depth,
                hops,
            });
        }
        let next = if first_class {
            class_link(receiver)?
        } else if pid == PROTO_ID_DEFAULT {
            object_prototype as *const ObjectHeader
        } else {
            next_from_word(current, word)
        };
        // Valid JS chains are acyclic. Decline malformed native chains rather
        // than imposing a depth limit on valid ones.
        if next.is_null()
            || next == receiver
            || hops.iter().any(|&(addr, _)| addr == next as usize)
            || !hop_admitted(next as usize)
        {
            return None;
        }
        let shape = object_shape_descriptor(next)?;
        let stamp = object_shape_stamp(next);
        if !shape.object_kind.is_ordinary_layout() || stamp == 0 {
            return None;
        }
        if key_may_be_accessor(next, name) {
            return None;
        }
        let keys = shape.keys as usize as *const crate::array::ArrayHeader;
        if !keys.is_null() {
            if let Some(slot) =
                crate::object::keys_find_slot_by_bytes_resolved(keys, shape.logical_key_count, name)
            {
                if crate::object::key_attrs::key_is_accessor_at(keys, slot) {
                    return None;
                }
                let slot = holder_slot_word(next as usize, slot, shape.live_inline_slot_count)?;
                return Some(Proof {
                    holder: next as usize,
                    holder_shape: stamp,
                    slot: Some(slot),
                    depth: hops.len() + 1,
                    hops,
                });
            }
        }
        hops.push((next as usize, stamp));
        current = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deep_runtime_alias_retains_generic_forwarding() {
        if !super::super::super::run_with_fresh_worker_gate(
            "deep_runtime_alias_retains_generic_forwarding",
        ) {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        crate::object::builtin_prototype_value("Object");
        unsafe {
            let key = crate::string::canonical_key(b"deep_alias_field");
            let holder = crate::object::js_object_alloc(0, 1);
            crate::object::js_object_set_field_by_name(holder, key, 7.0);
            let mut proto = holder;
            for _ in 0..6 {
                proto = crate::value::js_nanbox_get_pointer(crate::object::js_object_create(
                    crate::value::js_nanbox_pointer(proto as i64),
                )) as *mut ObjectHeader;
            }
            let receiver = crate::object::js_object_alloc(0xC000_2002, 0);
            crate::object::prototype_chain::object_set_user_prototype(
                receiver as usize,
                crate::value::js_nanbox_pointer(proto as i64).to_bits(),
            );
            crate::object::native_this_alias::register_this_to_handle_alias(
                crate::value::js_nanbox_pointer(receiver as i64),
                crate::value::js_nanbox_pointer(42),
                false,
            );
            assert_eq!(
                object_shape_descriptor(receiver).unwrap().object_kind,
                crate::object::shapes::ShapeObjectKind::OrdinaryNativeAlias,
            );
            let mut slot = std::ptr::null_mut();
            assert!(read_runtime_chain(receiver, b"deep_alias_field", &mut slot).is_none());
            assert!(slot.is_null());
        }
    }
}
