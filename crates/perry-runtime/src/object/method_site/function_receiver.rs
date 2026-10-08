use super::*;

/// Prime an own function-bag slot or an inherited slot on the holder named
/// by the function shape's prototype serial. FunctionDictionary declines;
/// base and keyed Function shapes prove the own key list and prototype.
/// The receiver's ShapeId pins these facts independently of capture count.
/// Own hits reload the bag slot; inherited hits use the shared holder path.
pub(super) unsafe fn prime_function(
    slot: *mut MethodSiteSlot,
    addr: usize,
    name: &[u8],
    argc: usize,
) {
    if !address_is_prime_stable(addr) {
        refuse(1);
        return;
    }
    let closure = addr as *const crate::closure::ClosureHeader;
    let id = (*closure).shape_id;
    if id == crate::closure::shape::function_dictionary_shape()
        || super::super::shapes::shape_object_kind_by_id(id)
            != Some(super::super::shapes::ShapeObjectKind::Function)
    {
        refuse(2);
        return;
    }
    let Some(shape) = super::super::shapes::shape_descriptor_by_id(id) else {
        refuse(1);
        return;
    };
    let keys = shape.keys as usize as *const crate::array::ArrayHeader;
    let own = if keys.is_null() {
        None
    } else {
        super::super::keys_find_slot_by_bytes_resolved(keys, shape.logical_key_count, name)
    };
    let Some(s) = own else {
        // The shape's implicit own data keys are not bag slots. They cannot
        // be skipped merely because the explicit key list lacks this key.
        if shape.implicit_own_keys().contains(&name) {
            refuse(19);
            return;
        }
        // The shape pins both absence and the intrinsic prototype serial.
        // Materialize that holder even before user code names Function.
        if shape.proto_id == crate::closure::shape::INTRINSIC_SERIAL_FUNCTION {
            let holder = crate::closure::shape::function_prototype_ptr_materialized();
            prime_holder(
                slot,
                holder as *const ObjectHeader,
                receiver_word(std::ptr::read(addr as *const u64)),
                name,
                argc,
            );
        }
        return;
    };
    let bag = crate::closure::props::bag_of(addr);
    if bag.is_null() {
        return;
    }
    if s >= shape.live_inline_slot_count {
        refuse(3);
        return;
    }
    let value = field_bits(bag as usize, s);
    let Some(info) = direct_callable(value, argc) else {
        refuse(5);
        return;
    };
    let entry = MethodEntry {
        word: receiver_word(std::ptr::read(addr as *const u64)),
        slot: s as u64 | METHOD_SITE_FUNCTION_BAG | native_args_tag(info),
        info: info as *const crate::closure::JsFunctionInfo as u64,
        code: info.code as u64,
        closure: 0,
        gen: 0,
    };
    if publish(slot, entry) {
        PRIMES_FUNCTION.fetch_add(1, Ordering::Relaxed);
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn target(
        _closure: *const crate::closure::ClosureHeader,
        _this: crate::closure::JsThis,
        a: f64,
    ) -> f64 {
        a
    }

    #[test]
    fn function_receiver_holder_hit_and_sabotage() {
        if !run_with_fresh_worker_gate("function_receiver_holder_hit_and_sabotage") {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let _no_move = crate::gc::GcSuppressScope::new();
            let f = crate::closure::js_closure_alloc(crate::fn_info!(target, 1), 0);
            let recv = crate::value::js_nanbox_pointer(f as i64);
            let mut slot: MethodSiteSlot = std::ptr::null_mut();
            for name in [b"bind".as_slice(), b"call", b"apply"] {
                prime(&mut slot, recv, name, 1);
                let hit = memo_hit(&mut slot, recv.to_bits()).expect("live inherited hit");
                let entry = &mut (*slot).entries[0];
                assert_ne!(entry.slot & METHOD_SITE_INHERITED, 0);
                assert_ne!(entry.closure, 0);
                assert!(closure_of_body(hit.0, entry.info));
                let other = crate::closure::js_closure_alloc(crate::fn_info!(target, 1), 3);
                assert!(
                    memo_hit(
                        &mut slot,
                        crate::value::js_nanbox_pointer(other as i64).to_bits()
                    )
                    .is_some(),
                    "capture count is not shape identity"
                );
                assert_eq!(
                    entry.slot & crate::codegen_abi::METHOD_SITE_NATIVE_ARGS != 0,
                    name != b"apply",
                    "the body ABI, not the key, chooses the call"
                );
                // Plant a broken receiver guard. A detector that only observes
                // successful dispatch would stay green via the fallback.
                let word = entry.word;
                entry.word = METHOD_SITE_EMPTY;
                assert!(memo_hit(&mut slot, recv.to_bits()).is_none());
                entry.word = word;
                assert!(memo_hit(&mut slot, recv.to_bits()).is_some());
                // Plant a wrong holder guard too: the prototype is essential.
                let gen = entry.gen;
                entry.gen ^= 1 << 32;
                assert!(memo_hit(&mut slot, recv.to_bits()).is_none());
                entry.gen = gen;
                assert!(memo_hit(&mut slot, recv.to_bits()).is_some());
            }
            let own = crate::closure::js_closure_alloc(crate::fn_info!(target, 1), 0);
            crate::closure::closure_define_dynamic_prop(
                f as usize,
                "bind",
                crate::value::js_nanbox_pointer(own as i64),
            );
            assert!(
                memo_hit(&mut slot, recv.to_bits()).is_none(),
                "own shadow revokes inherited hit"
            );
            prime(&mut slot, recv, b"bind", 1);
            let own_hit = memo_hit(&mut slot, recv.to_bits()).expect("own method hit");
            assert_eq!(
                own_hit.0,
                crate::value::js_nanbox_pointer(own as i64).to_bits()
            );
        }
    }
    #[test]
    fn function_implicit_own_key_refusal_is_counted() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let _no_move = crate::gc::GcSuppressScope::new();
            let f = crate::closure::js_closure_alloc(crate::fn_info!(target, 1), 0);
            let before = SITE_REFUSED[19].load(Ordering::Relaxed);
            let mut slot: MethodSiteSlot = std::ptr::null_mut();
            prime_function(&mut slot, f as usize, b"name", 0);
            assert!(slot.is_null());
            assert_eq!(SITE_REFUSED[19].load(Ordering::Relaxed), before + 1);
            assert_eq!(refusal_name(19), "function_implicit_own_key");
        }
    }
}
