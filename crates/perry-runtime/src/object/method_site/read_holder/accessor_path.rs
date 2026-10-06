//! Shape-pinned accessor walks shared by collecting reads and probes.
use super::*;

/// The object `recv`'s ShapeId pins as its direct prototype, for an accessor
/// entry: a declared class's prototype ([`class_link`]; `true`), or the object
/// a serial or default prototype identity names (`false`).
pub(super) unsafe fn accessor_holder(
    recv: *const ObjectHeader,
) -> Option<(*const ObjectHeader, bool)> {
    if let Some(holder) = class_link(recv) {
        return Some((holder, true));
    }
    let (pid, word) = admitted_link(recv)?;
    let holder = match pid {
        PROTO_ID_NULL => return None,
        PROTO_ID_DEFAULT => {
            crate::array::object_prototype_addr_if_resolved() as *const ObjectHeader
        }
        _ => next_from_word(recv, word),
    };
    (!holder.is_null() && holder != recv).then_some((holder, false))
}

/// Find the first property on a shape-pinned chain. An accessor answers;
/// data shadows it. Every intermediate shape proves absence and its link.
pub(crate) unsafe fn accessor_walk(
    recv: *const ObjectHeader,
    name: &[u8],
) -> Option<HolderAccessor> {
    if !holder_name_admitted(name)
        || crate::object::field_get_set::accessor_receiver_override_armed()
        || crate::object::prototype_chain::resolution_stack_savepoint() != 0
    {
        return None;
    }
    let (mut holder, _) = accessor_holder(recv)?;
    let mut hops = [(0, 0); HOLDER_MAX_DEPTH - 1];
    for depth in 1..=HOLDER_MAX_DEPTH {
        let addr = holder as usize;
        if !crate::value::addr_class::is_above_handle_band(addr)
            || !super::super::address_is_prime_stable(addr)
        {
            return None;
        }
        let header = crate::value::addr_class::try_read_gc_header(addr)?;
        if header.obj_type != crate::gc::GC_TYPE_OBJECT
            || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
            || header._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0
            || crate::object::dictionary::is_dictionary(holder)
        {
            return None;
        }
        let meta = (*holder).meta;
        if !meta.is_null()
            && ((*meta).elements != 0
                || (*meta).flags & crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER != 0)
        {
            return None;
        }
        let shape = object_shape_descriptor(holder)?;
        if !shape.object_kind.is_ordinary_layout() {
            return None;
        }
        let keys = shape.keys as usize as *const crate::array::ArrayHeader;
        let candidate = (shape.summary & crate::object::key_attrs::SUMMARY_ACCESSOR != 0)
            .then(|| {
                crate::object::key_attrs::keys_find_accessor_slot_resolved(
                    keys,
                    shape.logical_key_count,
                    name,
                )
            })
            .flatten();
        if let Some(slot) = candidate {
            // No getter ran while walking. Only a positive accessor needs
            // name lookups in the nearer holders to prove it is unshadowed.
            for &(hop, _) in &hops[..depth - 1] {
                let nearer = object_shape_descriptor(hop as *const ObjectHeader)?;
                if crate::object::keys_find_slot_by_bytes_resolved(
                    nearer.keys as usize as *const crate::array::ArrayHeader,
                    nearer.logical_key_count,
                    name,
                )
                .is_some()
                {
                    return None;
                }
            }
            let slot = holder_slot_word(addr, slot, shape.live_inline_slot_count)?;
            let lane = holder_slot_value(addr, slot)?;
            let getter = crate::object::accessor_pair::site_getter_word_of_value(lane)?;
            return Some(HolderAccessor {
                hops,
                depth,
                holder: addr,
                shape: object_shape_stamp(holder),
                slot,
                pair: (lane & crate::value::POINTER_MASK) as usize,
                getter,
            });
        }
        if depth == HOLDER_MAX_DEPTH {
            return None;
        }
        // Declared prototypes also carry MIXED identities: the class word
        // plus the serial of the explicitly recorded parent. That serial
        // pins one next holder, just like a plain object's recorded link.
        let pid = shape_proto_id(object_shape_stamp(holder))?;
        let (pid, word) = if (PROTO_ID_MIXED..PROTO_ID_UNIQUE).contains(&pid) {
            let (stated, word) = stated_link(holder);
            if stated != pid {
                return None;
            }
            (pid, word)
        } else {
            admitted_link(holder)?
        };
        if pid == PROTO_ID_NULL {
            return None;
        }
        hops[depth - 1] = (addr, object_shape_stamp(holder));
        let next = if pid == PROTO_ID_DEFAULT {
            crate::array::object_prototype_addr_if_resolved() as *const ObjectHeader
        } else {
            next_from_word(holder, word)
        };
        if next.is_null() || next == holder || next == recv {
            return None;
        }
        holder = next;
    }
    None
}
