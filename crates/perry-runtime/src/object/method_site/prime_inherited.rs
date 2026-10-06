use super::*;

/// Prime an inherited entry when the receiver's shape pins a direct holder
/// with `name` in a plain inline data slot. Deeper chains use ordinary dispatch.
pub(super) unsafe fn prime_inherited(
    slot: *mut MethodSiteSlot,
    obj: *const ObjectHeader,
    word: u64,
    name: &[u8],
    argc: usize,
) {
    if WORKER_AGENTS_EXIST.load(Ordering::SeqCst) != 0 {
        refuse(11);
        return;
    }
    // A declared-class instance's direct prototype is its class's prototype
    // object (a bare CLASS identity: the class's function object keeps that
    // link for the agent's life, and a relink retires the displaced
    // prototype's ShapeId) or the serial a MIXED identity records. Its
    // methods are real slots of that object (class prototypes hold function
    // objects of their bodies, with ConstFn lanes), so the entry is the same
    // holder entry as for any receiver.
    let class_id = (*obj).class_id;
    let class_instance = class_id != 0
        && class_id < crate::object::class_registry::prototype_objects::SYNTHETIC_CLASS_ID_BASE
        && !crate::object::is_anon_shape_class_id(class_id);
    if key_may_be_accessor(obj, name) {
        refuse(8);
        return;
    }
    let class_holder = if class_instance {
        match read_holder::class_link(obj) {
            Some(holder) => Some(holder),
            None => {
                refuse(6);
                return;
            }
        }
    } else {
        None
    };
    // Otherwise only a serial or the realm-default identity pins one direct
    // prototype.
    let proto_id = match class_holder {
        Some(_) => crate::object::shapes::PROTO_ID_CLASS,
        None => match read_holder::admitted_proto_id(obj) {
            Some(pid) => pid,
            None => {
                refuse(7);
                return;
            }
        },
    };
    {
        let next = if let Some(holder) = class_holder {
            holder
        } else if proto_id == crate::object::shapes::PROTO_ID_DEFAULT {
            crate::array::object_prototype_addr_if_resolved() as *const ObjectHeader
        } else {
            next_prototype(obj)
        };
        if next.is_null() || next == obj {
            refuse(9);
            return;
        }
        let next_addr = next as usize;
        if !crate::value::addr_class::is_above_handle_band(next_addr)
            || !address_is_prime_stable(next_addr)
        {
            refuse(8);
            return;
        }
        let Some(header) = crate::value::addr_class::try_read_gc_header(next_addr) else {
            refuse(8);
            return;
        };
        if header.obj_type != crate::gc::GC_TYPE_OBJECT
            || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
            || header._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0
            || crate::object::dictionary::is_dictionary(next)
        {
            refuse(8);
            return;
        }
        let Some(shape) = crate::object::shapes::object_shape_descriptor(next) else {
            refuse(8);
            return;
        };
        if !shape.object_kind.is_ordinary_layout()
            || crate::object::shapes::object_shape_stamp(next) == 0
        {
            refuse(8);
            return;
        }
        let meta = (*next).meta;
        if (!meta.is_null()
            && ((*meta).elements != 0
                || (*meta).flags & crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER != 0))
            || key_may_be_accessor(next, name)
        {
            refuse(8);
            return;
        }
        let keys = shape.keys as usize as *const crate::array::ArrayHeader;
        if !keys.is_null() {
            if let Some(s) =
                crate::object::keys_find_slot_by_bytes_resolved(keys, shape.logical_key_count, name)
            {
                if s >= shape.live_inline_slot_count {
                    refuse(8);
                    return;
                }
                let value = field_bits(next_addr, s);
                // `%Object.prototype%.hasOwnProperty` and the other builtin
                // methods are ordinary slots of their prototype (born wide,
                // `global_this/proto_room.rs`): the hit loads the slot and
                // compares the body, so a replaced builtin is seen at once.
                let Some(info) = direct_callable(value, argc) else {
                    refuse(10);
                    return;
                };
                // A holder whose shape owns this slot's body (ConstFn) lets
                // the hit call the body after the two word compares, with no
                // kind or info check of the slot value: the holder word pins
                // the holder's shape and that shape pins the body.
                let constfn = if s < crate::object::field_rep::REP_SLOTS
                    && shape.special_constfn_mask & (1 << s) != 0
                {
                    let body = shape
                        .constfn_infos()
                        .iter()
                        .find(|entry| u32::from(entry.slot) == s)
                        .map(|entry| entry.info);
                    if body != Some(info as *const crate::closure::JsFunctionInfo as u64) {
                        refuse(17);
                        return;
                    }
                    if declares_at_most(info, argc) {
                        METHOD_SITE_CONSTFN
                    } else {
                        0
                    }
                } else {
                    0
                };
                let entry = MethodEntry {
                    word,
                    slot: METHOD_SITE_INHERITED | constfn | u64::from(s),
                    info: info as *const crate::closure::JsFunctionInfo as u64,
                    code: info.code as u64,
                    closure: next_addr,
                    gen: std::ptr::read(next_addr as *const u64),
                };
                if publish(slot, entry) {
                    PRIMES_INHERITED.fetch_add(1, Ordering::Relaxed);
                    note_builtin_prime(info);
                    if constfn != 0 {
                        PRIMES_CONSTFN.fetch_add(1, Ordering::Relaxed);
                    }
                }
                return;
            }
        }
    }
    refuse(9);
}
