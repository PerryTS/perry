use super::*;

use read_holder::shared::SharedEntry as MethodChain;

/// Retain the normal function-value invocation for bound/rest/capture bodies;
/// the property answer is still loaded under the same shape proof.
unsafe extern "C" fn call_value_args(
    closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    args: *const f64,
    len: usize,
) -> f64 {
    let value = crate::value::js_nanbox_pointer(closure as i64);
    // The hit has already validated this closure. A baked receiver that is
    // already the call receiver needs neither another value check nor a clone.
    if crate::closure::closure_reads_this_from_capture(closure) {
        let count = crate::closure::real_capture_count((*closure).capture_count) as usize;
        let captures = crate::closure::closure_capture_slots_mut(closure.cast_mut());
        if *captures.add(count - 1) != this.as_f64().to_bits() {
            return js_method_site_call_value(value, this.as_f64(), args, len);
        }
    }
    let info = &*(*closure).info;
    if info.flags & crate::codegen_abi::FN_COMPILED_BODY != 0 {
        // The site proved the live closure, and binding above prepared this.
        // Reuse the body dispatcher rather than checking exotic value kinds.
        return crate::closure::call_compiled_body_this(closure, info, this, args, len);
    }
    crate::closure::native_call_value_this(value, this, args, len)
}

/// Reuse the shape's slot answer when its current callable body changes.
/// The existing generic invocation format needs no saved body identity.
pub(super) fn unify_own_body(old: &MethodEntry, new: &mut MethodEntry) -> bool {
    let args = crate::codegen_abi::METHOD_SITE_NATIVE_ARGS;
    if old.word != new.word || old.slot & !args != new.slot & !args {
        return false;
    }
    if old.info == METHOD_SITE_VALUE_INFO {
        return true;
    }
    if old.info != new.info && new.slot & METHOD_SITE_CONSTFN == 0 {
        new.info = METHOD_SITE_VALUE_INFO;
        new.code = call_value_args as *const u8 as u64;
        new.slot |= args;
    }
    false
}

pub(super) unsafe fn callable_route(
    value: u64,
    argc: usize,
) -> Option<(&'static crate::closure::JsFunctionInfo, u64, u64)> {
    if let Some(info) = direct_callable(value, argc) {
        return Some((info, info.code as u64, native_args_tag(info)));
    }
    if value & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return None;
    }
    let addr = (value & crate::value::POINTER_MASK) as usize;
    if !crate::closure::is_closure_ptr(addr) || !address_is_prime_stable(addr) {
        return None;
    }
    let info = (*(addr as *const crate::closure::ClosureHeader))
        .info
        .as_ref()?;
    if info.code.is_null()
        || super::super::class_registry::is_class_object_value(f64::from_bits(value))
    {
        return None;
    }
    Some((
        info,
        call_value_args as *const u8 as u64,
        crate::codegen_abi::METHOD_SITE_NATIVE_ARGS,
    ))
}

// A failed direct-holder prime is cold. Keep its complete chain walk out
// of the direct-holder frame and retain a symbol for external entry counts.
#[cold]
#[inline(never)]
pub(super) unsafe fn prime_chain(
    slot: *mut MethodSiteSlot,
    recv: *const ObjectHeader,
    word: u64,
    name: &[u8],
    argc: usize,
) -> bool {
    use crate::object::native_call_method::class_holder::{chain_method, ChainMethod, MethodKey};
    let start = read_holder::class_link(recv).or_else(|| {
        let (pid, link) = read_holder::admitted_link(recv)?;
        Some(if pid == crate::object::shapes::PROTO_ID_DEFAULT {
            crate::array::object_prototype_addr_if_resolved() as *const ObjectHeader
        } else {
            read_holder::next_from_word(recv, link)
        })
    });
    let Some(start) = start.filter(|p| !p.is_null()) else {
        return false;
    };
    let ChainMethod::Data {
        value,
        holder,
        slot: inline,
        ..
    } = chain_method(start, &MethodKey::bytes(name))
    else {
        return false;
    };
    let index = if let Some(index) = inline {
        index
    } else {
        let Some(shape) = crate::object::shapes::object_shape_descriptor(holder) else {
            return false;
        };
        let keys = shape.keys as usize as *const crate::array::ArrayHeader;
        let Some(index) =
            crate::object::keys_find_slot_by_bytes_resolved(keys, shape.logical_key_count, name)
        else {
            return false;
        };
        index | (1 << 31)
    };
    let Some((info, code, tag)) = callable_route(value, argc) else {
        return false;
    };
    let Some(links) = crate::object::shape_chain::ShapeChain::capture(recv) else {
        return false;
    };
    let chain = Box::into_raw(Box::new(MethodChain::method(links, holder as usize, index)));
    let entry = MethodEntry {
        word,
        slot: METHOD_SITE_INHERITED | METHOD_SITE_CHAIN | tag,
        info: if code == info.code as u64 {
            info as *const _ as u64
        } else {
            METHOD_SITE_VALUE_INFO
        },
        closure: chain as usize,
        gen: 0,
        code,
    };
    if publish(slot, entry) {
        PRIMES_INHERITED.fetch_add(1, Ordering::Relaxed);
        true
    } else {
        (*chain).drop_method_proof();
        drop(Box::from_raw(chain));
        false
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_method_site_chain_value(entry: *const MethodEntry) -> u64 {
    let entry = &*entry;
    let chain = &*(entry.closure as *const MethodChain);
    chain.method_value().unwrap_or(crate::value::TAG_HOLE)
}

pub(super) unsafe fn drop_chain(entry: &MethodEntry) {
    if entry.slot & METHOD_SITE_CHAIN != 0 && entry.closure != 0 {
        let chain = Box::from_raw(entry.closure as *mut MethodChain);
        chain.drop_method_proof();
    }
}

pub(super) unsafe fn scan_chain(
    entry: &mut MethodEntry,
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
) {
    let chain = &mut *(entry.closure as *mut MethodChain);
    chain.scan(visitor);
}

/// A direct-holder lookup only asks for a complete proof when a deeper or
/// extended callable route can still answer. Refusal is returned, never stored.
pub(super) enum HolderPrime {
    Published,
    Refused,
    NeedsChain,
}

pub(super) unsafe fn prime_holder(
    slot: *mut MethodSiteSlot,
    next: *const ObjectHeader,
    word: u64,
    name: &[u8],
    argc: usize,
) -> HolderPrime {
    {
        if next.is_null() {
            refuse(9);
            return HolderPrime::Refused;
        }
        let next_addr = next as usize;
        if !crate::value::addr_class::is_above_handle_band(next_addr)
            || !address_is_prime_stable(next_addr)
        {
            refuse(8);
            return HolderPrime::Refused;
        }
        let Some(header) = crate::value::addr_class::try_read_gc_header(next_addr) else {
            refuse(8);
            return HolderPrime::Refused;
        };
        if header.obj_type != crate::gc::GC_TYPE_OBJECT
            || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
            || header._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0
            || super::super::dictionary::is_dictionary(next)
        {
            refuse(8);
            return HolderPrime::Refused;
        }
        let Some(shape) = super::super::shapes::object_shape_descriptor(next) else {
            refuse(8);
            return HolderPrime::Refused;
        };
        if !shape.object_kind.is_ordinary_layout()
            || super::super::shapes::object_shape_stamp(next) == 0
        {
            refuse(8);
            return HolderPrime::Refused;
        }
        let meta = (*next).meta;
        if !meta.is_null()
            && ((*meta).elements != 0
                || (*meta).flags
                    & (super::super::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER
                        | super::super::OBJECT_META_FLAG_NATIVE_ALIAS)
                    != 0)
        {
            refuse(8);
            return HolderPrime::Refused;
        }
        let keys = shape.keys as usize as *const crate::array::ArrayHeader;
        if !keys.is_null() {
            if let Some(s) =
                super::super::keys_find_slot_by_bytes_resolved(keys, shape.logical_key_count, name)
            {
                if super::super::key_attrs::key_is_accessor_at(keys, s) {
                    refuse(8);
                    return HolderPrime::Refused;
                }
                if s >= shape.live_inline_slot_count {
                    refuse(8);
                    return HolderPrime::NeedsChain;
                }
                let value = field_bits(next_addr, s);
                // `%Object.prototype%.hasOwnProperty` and the other builtin
                // methods are ordinary slots of their prototype (born wide,
                // `global_this/proto_room.rs`): the hit loads the slot and
                // compares the body, so a replaced builtin is seen at once.
                let Some(info) = direct_callable(value, argc) else {
                    refuse(10);
                    // The complete route can serve captured/rest/bound callables.
                    return HolderPrime::NeedsChain;
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
                        return HolderPrime::Refused;
                    }
                    if native_args_tag(info) == 0 && declares_at_most(info, argc) {
                        METHOD_SITE_CONSTFN
                    } else {
                        0
                    }
                } else {
                    0
                };
                // The two ShapeIds may also answer the body's own lookups.
                let code = match constfn {
                    0 => info.code as u64,
                    _ => crate::object::regex_proto_thunks::method_site_code(
                        info, word, &shape, argc,
                    ),
                };
                let entry = MethodEntry {
                    word,
                    slot: METHOD_SITE_INHERITED | constfn | native_args_tag(info) | u64::from(s),
                    info: info as *const crate::closure::JsFunctionInfo as u64,
                    code,
                    closure: next_addr,
                    gen: std::ptr::read(next_addr as *const u64),
                };
                let published = publish(slot, entry);
                if published {
                    PRIMES_INHERITED.fetch_add(1, Ordering::Relaxed);
                    note_builtin_prime(info);
                    if constfn != 0 {
                        PRIMES_CONSTFN.fetch_add(1, Ordering::Relaxed);
                    }
                }
                return if published {
                    HolderPrime::Published
                } else {
                    HolderPrime::Refused
                };
            }
        }
        refuse(9);
        // Absence at a null link is the complete answer. Searching the same
        // terminal holder again cannot find a deeper callable. The shape
        // supplies this verdict; no site remembers a refusal.
        if shape.proto_id == crate::object::shapes::PROTO_ID_NULL {
            HolderPrime::Refused
        } else {
            HolderPrime::NeedsChain
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn body(
        _closure: *const crate::closure::ClosureHeader,
        _this: crate::closure::JsThis,
    ) -> f64 {
        7.0
    }

    extern "C" fn mutating_body(
        _closure: *const crate::closure::ClosureHeader,
        this: crate::closure::JsThis,
    ) -> f64 {
        let key = crate::string::canonical_key(b"method_body_added");
        let object = crate::value::js_nanbox_get_pointer(this.as_f64()) as *mut ObjectHeader;
        crate::object::js_object_set_field_by_name(object, key, 3.0);
        7.0
    }

    #[test]
    fn a_mutating_body_primes_the_shape_seen_before_the_call() {
        if !run_with_fresh_worker_gate("a_mutating_body_primes_the_shape_seen_before_the_call") {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        unsafe {
            let key = crate::string::canonical_key(b"mutating_method");
            let holder = crate::object::js_object_alloc(0, 1);
            let value = crate::value::js_nanbox_pointer(crate::closure::js_closure_alloc(
                crate::fn_info!(mutating_body, 0),
                0,
            ) as i64);
            crate::object::js_object_set_field_by_name(holder, key, value);
            let a = crate::object::js_object_alloc(0, 4);
            let b = crate::object::js_object_alloc(0, 4);
            for recv in [a, b] {
                crate::object::prototype_chain::object_set_user_prototype(
                    recv as usize,
                    crate::value::js_nanbox_pointer(holder as i64).to_bits(),
                );
            }
            let word = std::ptr::read(a as *const u64);
            assert_eq!(word, std::ptr::read(b as *const u64));
            let mut slot: MethodSiteSlot = std::ptr::null_mut();
            let id = crate::value::JSValue::string_ptr(key as *mut _).bits() as i64;
            assert_eq!(
                js_method_site_miss(
                    &mut slot,
                    0,
                    crate::value::js_nanbox_pointer(a as i64),
                    id,
                    std::ptr::null(),
                    0,
                ),
                7.0
            );
            assert_ne!(word, std::ptr::read(a as *const u64));
            assert!(!slot.is_null(), "the pre-call shape was admitted");
            assert!((*slot).entries.iter().any(|entry| entry.word == word));
            assert!(memo_hit(
                &mut slot,
                crate::value::js_nanbox_pointer(b as i64).to_bits()
            )
            .is_some());
        }
    }

    #[test]
    fn method_chain_layout_matches_the_emitted_loads() {
        assert_eq!(std::mem::offset_of!(MethodChain, hops), 0);
        assert_eq!(
            std::mem::offset_of!(MethodChain, holder),
            2 * std::mem::size_of::<usize>()
        );
        assert_eq!(
            std::mem::offset_of!(MethodChain, slot),
            3 * std::mem::size_of::<usize>()
        );
        #[cfg(target_pointer_width = "64")]
        {
            assert_eq!(
                std::mem::offset_of!(MethodChain, holder),
                crate::codegen_abi::METHOD_CHAIN_HOLDER_OFFSET
            );
            assert_eq!(
                std::mem::offset_of!(MethodChain, slot),
                crate::codegen_abi::METHOD_CHAIN_SLOT_OFFSET
            );
        }
    }

    #[test]
    fn deep_method_entries_prime_and_guard_shadowing_and_trace_the_chain() {
        if !run_with_fresh_worker_gate(
            "deep_method_entries_prime_and_guard_shadowing_and_trace_the_chain",
        ) {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        crate::object::builtin_prototype_value("Object");
        unsafe {
            let name = b"deep_method_entry";
            let key = crate::string::canonical_key(name);
            let holder = crate::object::js_object_alloc(0, 1);
            let value = crate::value::js_nanbox_pointer(crate::closure::js_closure_alloc(
                crate::fn_info!(body, 0),
                0,
            ) as i64);
            crate::object::js_object_set_field_by_name(holder, key, value);
            let middle =
                crate::object::js_object_create(crate::value::js_nanbox_pointer(holder as i64));
            let middle = crate::value::js_nanbox_get_pointer(middle) as *mut ObjectHeader;
            let receiver =
                crate::object::js_object_create(crate::value::js_nanbox_pointer(middle as i64));
            let receiver = crate::value::js_nanbox_get_pointer(receiver) as *mut ObjectHeader;
            let mut slot: MethodSiteSlot = std::ptr::null_mut();
            assert!(prime_chain(
                &mut slot,
                receiver,
                std::ptr::read(receiver as *const u64),
                name,
                0
            ));
            let entry = &mut (*slot).entries[0];
            assert_ne!(entry.slot & METHOD_SITE_CHAIN, 0);
            assert_eq!(js_method_site_chain_value(entry), value.to_bits());
            let mut traced = Vec::new();
            scan_chain(
                entry,
                &mut crate::gc::RuntimeRootVisitor::for_copy(&mut |v| {
                    traced.push(v.to_bits() & crate::value::POINTER_MASK)
                }),
            );
            assert!(traced.contains(&(holder as u64)));
            assert!(traced.contains(&(middle as u64)));
            crate::object::js_object_set_field_by_name(middle, key, value);
            assert_eq!(js_method_site_chain_value(entry), crate::value::TAG_HOLE);
        }
    }

    extern "C" fn other_body(
        _closure: *const crate::closure::ClosureHeader,
        _this: crate::closure::JsThis,
    ) -> f64 {
        9.0
    }

    #[test]
    fn value_method_entry_serves_changed_bodies_without_eviction_and_declines_noncallables() {
        if !run_with_fresh_worker_gate(
            "value_method_entry_serves_changed_bodies_without_eviction_and_declines_noncallables",
        ) {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        crate::object::builtin_prototype_value("Object");
        unsafe {
            let name = b"value_method_body";
            let key = crate::string::canonical_key(name);
            let receiver = crate::object::js_object_alloc(0, 0);
            let bits = crate::value::js_nanbox_pointer(receiver as i64);
            let captured = crate::closure::js_closure_alloc(
                crate::fn_info!(body, 0),
                crate::closure::CAPTURES_THIS_FLAG | 1,
            );
            crate::closure::js_closure_set_capture_bits(captured, 0, bits.to_bits());
            let first = crate::value::js_nanbox_pointer(captured as i64);
            crate::object::js_object_set_field_by_name(receiver, key, first);
            let word = std::ptr::read(receiver as *const u64);
            let mut slot: MethodSiteSlot = std::ptr::null_mut();
            prime(&mut slot, bits, name, 0);
            let (value, code) =
                memo_hit(&mut slot, bits.to_bits()).expect("captured method primed");
            assert_eq!(code, 0, "the property value uses generic invocation");
            assert_eq!(
                js_method_site_call_value(f64::from_bits(value), bits, std::ptr::null(), 0),
                7.0
            );
            let second = crate::value::js_nanbox_pointer(crate::closure::js_closure_alloc(
                crate::fn_info!(other_body, 0),
                0,
            ) as i64);
            for i in 0..32 {
                let method = if i % 2 == 0 { second } else { first };
                crate::object::js_object_set_field_by_name(receiver, key, method);
                assert_eq!(std::ptr::read(receiver as *const u64), word);
                let (value, code) =
                    memo_hit(&mut slot, bits.to_bits()).expect("body-independent value hit");
                assert_eq!(value, method.to_bits());
                assert_eq!(code, 0);
                assert_eq!(
                    js_method_site_call_value(f64::from_bits(value), bits, std::ptr::null(), 0),
                    if i % 2 == 0 { 9.0 } else { 7.0 }
                );
            }
            assert_eq!(
                (*slot).next,
                0,
                "changing bodies never evicts a property-value entry"
            );
            crate::object::js_object_set_field_by_name(receiver, key, 51.0);
            assert_eq!(std::ptr::read(receiver as *const u64), word);
            assert!(
                memo_hit(&mut slot, bits.to_bits()).is_none(),
                "noncallables retain ordinary dispatch"
            );
        }
    }
    #[test]
    fn direct_method_body_changes_widen_the_existing_slot_instead_of_consuming_ways() {
        if !run_with_fresh_worker_gate(
            "direct_method_body_changes_widen_the_existing_slot_instead_of_consuming_ways",
        ) {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        unsafe {
            let name = b"direct_value_method_body";
            let key = crate::string::canonical_key(name);
            let receiver = crate::object::js_object_alloc(0, 0);
            let bits = crate::value::js_nanbox_pointer(receiver as i64);
            let first = crate::value::js_nanbox_pointer(crate::closure::js_closure_alloc(
                crate::fn_info!(body, 0),
                0,
            ) as i64);
            let second = crate::value::js_nanbox_pointer(crate::closure::js_closure_alloc(
                crate::fn_info!(other_body, 0),
                0,
            ) as i64);
            crate::object::js_object_set_field_by_name(receiver, key, first);
            let word = std::ptr::read(receiver as *const u64);
            let mut slot: MethodSiteSlot = std::ptr::null_mut();
            prime(&mut slot, bits, name, 0);
            assert!(!slot.is_null());
            assert_ne!((*slot).entries[0].info, 0, "first observation is direct");
            crate::object::js_object_set_field_by_name(receiver, key, second);
            assert_eq!(std::ptr::read(receiver as *const u64), word);
            prime(&mut slot, bits, name, 0);
            assert_eq!(
                (*slot).entries[0].info,
                METHOD_SITE_VALUE_INFO,
                "the slot answer widens"
            );
            for value in [first, second].into_iter().cycle().take(32) {
                crate::object::js_object_set_field_by_name(receiver, key, value);
                let (loaded, code) = memo_hit(&mut slot, bits.to_bits()).unwrap();
                assert_eq!(loaded, value.to_bits());
                assert_eq!(code, 0);
            }
            assert_eq!((*slot).next, 0);
            assert_eq!((*slot).entries.iter().filter(|e| e.word == word).count(), 1);
        }
    }
    extern "C" fn closure_identity(
        closure: *const crate::closure::ClosureHeader,
        _this: crate::closure::JsThis,
    ) -> f64 {
        crate::value::js_nanbox_pointer(closure as i64)
    }

    #[test]
    fn value_method_invocation_reuses_matching_this_and_rebinds_a_different_receiver() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        unsafe {
            let first =
                crate::value::js_nanbox_pointer(crate::object::js_object_alloc(0, 0) as i64);
            let second =
                crate::value::js_nanbox_pointer(crate::object::js_object_alloc(0, 0) as i64);
            for info in [
                crate::fn_info!(closure_identity, 0),
                crate::fn_info!(closure_identity, 0; with_flags(crate::codegen_abi::FN_COMPILED_BODY)),
            ] {
                let closure =
                    crate::closure::js_closure_alloc(info, crate::closure::CAPTURES_THIS_FLAG | 1);
                crate::closure::js_closure_set_capture_bits(closure, 0, first.to_bits());
                let value = crate::value::js_nanbox_pointer(closure as i64);
                let same = call_value_args(
                    closure,
                    crate::closure::JsThis::from_f64(first),
                    std::ptr::null(),
                    0,
                );
                assert_eq!(
                    same.to_bits(),
                    value.to_bits(),
                    "matching this keeps the environment"
                );
                let rebound = call_value_args(
                    closure,
                    crate::closure::JsThis::from_f64(second),
                    std::ptr::null(),
                    0,
                );
                assert_ne!(rebound.to_bits(), value.to_bits());
                let clone = crate::value::js_nanbox_get_pointer(rebound)
                    as *const crate::closure::ClosureHeader;
                assert_eq!(
                    crate::closure::js_closure_get_capture_bits(clone, 0),
                    second.to_bits()
                );
                assert_eq!(
                    crate::closure::js_closure_get_capture_bits(closure, 0),
                    first.to_bits()
                );
            }
            let arrow = crate::closure::js_closure_alloc(
                crate::fn_info!(closure_identity, 0; with_flags(crate::closure::FN_ARROW)),
                crate::closure::CAPTURES_THIS_FLAG | 1,
            );
            crate::closure::js_closure_set_capture_bits(arrow, 0, first.to_bits());
            assert_eq!(
                call_value_args(
                    arrow,
                    crate::closure::JsThis::from_f64(second),
                    std::ptr::null(),
                    0,
                )
                .to_bits(),
                crate::value::js_nanbox_pointer(arrow as i64).to_bits()
            );
            assert_eq!(
                crate::closure::js_closure_get_capture_bits(arrow, 0),
                first.to_bits()
            );
        }
    }
}
