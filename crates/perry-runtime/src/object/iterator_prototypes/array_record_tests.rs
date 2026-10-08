use super::*;
use crate::value::js_nanbox_pointer;

unsafe fn fixture() -> (f64, f64, f64) {
    let _stable = crate::gc::GcSuppressScope::new();
    build_iterator_prototypes();
    let proto = crate::object::builtin_prototype_value("Array");
    let sym = js_nanbox_pointer(crate::symbol::well_known_symbol("iterator") as i64);
    let original = crate::symbol::js_object_get_symbol_property(proto, sym);
    let obj =
        crate::object::shaped_symbols::owner(crate::value::js_nanbox_get_pointer(proto) as usize)
            .unwrap();
    // Other runtime tests restore JS values after mutation, which leaves
    // ConstFn revoked. Isolate this test's entry fixture explicitly.
    super::super::shapes::learn_object_constfn_lanes(obj, |_, bits| bits == original.to_bits());
    let array = js_nanbox_pointer(crate::array::js_array_alloc(0) as i64);
    let addr = crate::array::array_prototype_addr();
    let proto_shape = super::super::shapes::object_shape_record(obj).unwrap();
    let own_slots =
        crate::array::array_elements_ptr(proto_shape.keys() as *const crate::array::ArrayHeader);
    let entries = (0..proto_shape.logical_key_count())
        .map(|i| (i, *own_slots.add(i as usize), proto_shape.constfn_info(i)))
        .collect::<Vec<_>>();
    let next_proto = ARRAY_ITERATOR_PROTOTYPE_PTR.load(Ordering::Acquire) as *const ObjectHeader;
    let next_shape = super::super::shapes::object_shape_record(next_proto).unwrap();
    let next_slots =
        crate::array::array_elements_ptr(next_shape.keys() as *const crate::array::ArrayHeader);
    assert_eq!(crate::array::js_array_record_needs_iterator(array), 0,
        "entry fixture must be eligible: proto={:x} addr={addr:x} entries={entries:?} next_builtin={} next_key={:x} atom_key={:x} next_info={:?}",
        obj as usize, array_record_next_is_builtin(), *next_slots,
        JSValue::string_ptr(crate::string::intern_ascii_literal(b"next") as *mut crate::StringHeader).bits(),
        next_shape.constfn_info(0));
    (array, proto, sym)
}

#[test]
fn array_record_shape_carrier_survives_growth_and_rejects_wrong_body() {
    unsafe {
        let _stable = crate::gc::GcSuppressScope::new();
        let (array, proto, sym) = fixture();
        let raw = crate::value::js_nanbox_get_pointer(array) as *mut crate::array::ArrayHeader;
        crate::symbol::js_object_set_symbol_property(array, sym, 33.0);
        let owner = crate::object::shaped_symbols::owner(raw as usize).unwrap();
        assert!(super::super::shapes::object_shape_record(owner).is_some());
        let grown = crate::array::js_array_grow(crate::array::clean_arr_ptr_mut(raw), 100);
        assert_eq!(
            crate::symbol::js_object_get_symbol_property(js_nanbox_pointer(grown as i64), sym),
            33.0
        );
        let original = crate::symbol::js_object_get_symbol_property(proto, sym);
        let next_proto =
            ARRAY_ITERATOR_PROTOTYPE_PTR.load(Ordering::Acquire) as *const ObjectHeader;
        let wrong = f64::from_bits(super::super::js_object_get_field(next_proto, 0).bits());
        crate::symbol::js_object_set_symbol_property(proto, sym, wrong);
        let bag = crate::object::shaped_symbols::owner(
            crate::value::js_nanbox_get_pointer(proto) as usize
        )
        .unwrap();
        super::super::shapes::learn_object_constfn_lanes(bag, |_, bits| bits == wrong.to_bits());
        let fresh = js_nanbox_pointer(crate::array::js_array_alloc(0) as i64);
        assert_eq!(crate::array::js_array_record_needs_iterator(fresh), 1);
        crate::symbol::js_object_set_symbol_property(proto, sym, original);
    }
}

#[test]
fn array_record_absent_close_proof_revokes() {
    unsafe {
        let _stable = crate::gc::GcSuppressScope::new();
        fixture();
        assert!(array_record_close_is_absent());
        let proto = ARRAY_ITERATOR_PROTOTYPE_PTR.load(Ordering::Acquire) as *mut ObjectHeader;
        let key = crate::string::intern_ascii_literal(b"return");
        super::super::js_object_set_field_by_name(proto, key, 1.0);
        assert!(!array_record_close_is_absent());
    }
}

#[test]
fn array_record_alias_guard_reads_the_live_shape() {
    unsafe {
        let _stable = crate::gc::GcSuppressScope::new();
        let (array, _, sym) = fixture();
        let raw = crate::value::js_nanbox_get_pointer(array) as *mut crate::array::ArrayHeader;
        let grown = crate::array::js_array_grow(raw, 100);
        assert_ne!(raw, grown);
        crate::symbol::js_object_set_symbol_property(js_nanbox_pointer(grown as i64), sym, 33.0);
        assert_eq!(crate::array::js_array_record_needs_iterator(array), 1);
    }
}

#[test]
fn array_record_other_members_keep_the_array_representation() {
    unsafe {
        let _stable = crate::gc::GcSuppressScope::new();
        let (array, _, _) = fixture();
        let key = crate::string::intern_ascii_literal(b"unrelated");
        let raw = crate::value::js_nanbox_get_pointer(array) as *mut crate::array::ArrayHeader;
        crate::array::array_named_property_set(raw, key, 42.0);
        let symbol = js_nanbox_pointer(crate::symbol::well_known_symbol("toStringTag") as i64);
        crate::symbol::js_object_set_symbol_property(array, symbol, 99.0);
        assert_eq!(crate::array::js_array_record_needs_iterator(array), 0);
    }
}

#[test]
fn array_record_own_shape_guard() {
    unsafe {
        let _stable = crate::gc::GcSuppressScope::new();
        let (array, _, sym) = fixture();
        crate::symbol::js_object_set_symbol_property(
            array,
            sym,
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
        assert_eq!(crate::array::js_array_record_needs_iterator(array), 1);
    }
}

#[test]
fn array_record_prototype_member_guard() {
    unsafe {
        let _stable = crate::gc::GcSuppressScope::new();
        let (array, proto, sym) = fixture();
        let original = crate::symbol::js_object_get_symbol_property(proto, sym);
        crate::symbol::js_object_set_symbol_property(
            proto,
            sym,
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
        assert_eq!(crate::array::js_array_record_needs_iterator(array), 1);
        crate::symbol::js_object_set_symbol_property(proto, sym, original);
    }
}

#[test]
fn array_record_next_accessor_shape_guard() {
    unsafe {
        let _stable = crate::gc::GcSuppressScope::new();
        let (array, _, _) = fixture();
        let proto = ARRAY_ITERATOR_PROTOTYPE_PTR.load(Ordering::Acquire) as *mut ObjectHeader;
        let getter = super::super::js_object_get_field(proto, 0);
        let key = crate::string::intern_ascii_literal(b"next");
        super::super::js_object_define_accessor(
            js_nanbox_pointer(proto as i64),
            f64::from_bits(JSValue::string_ptr(key as *mut crate::StringHeader).bits()),
            f64::from_bits(getter.bits()),
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
        assert_eq!(crate::array::js_array_record_needs_iterator(array), 1);
    }
}

#[test]
fn array_record_next_constfn_body_guard() {
    unsafe {
        let _stable = crate::gc::GcSuppressScope::new();
        let (array, _, _) = fixture();
        let proto = ARRAY_ITERATOR_PROTOTYPE_PTR.load(Ordering::Acquire) as *mut ObjectHeader;
        let other = MAP_ITERATOR_PROTOTYPE_PTR.load(Ordering::Acquire) as *const ObjectHeader;
        let wrong = super::super::js_object_get_field(other, 0);
        super::super::js_object_set_field(proto, 0, wrong);
        super::super::shapes::learn_object_constfn_lanes(proto, |slot, bits| {
            slot == 0 && bits == wrong.bits()
        });
        assert!(super::super::shapes::object_shape_record(proto)
            .unwrap()
            .constfn_info(0)
            .is_some());
        assert_eq!(crate::array::js_array_record_needs_iterator(array), 1);
    }
}

#[test]
fn array_record_next_shape_guard() {
    unsafe {
        let _stable = crate::gc::GcSuppressScope::new();
        let (array, _, _) = fixture();
        let proto = ARRAY_ITERATOR_PROTOTYPE_PTR.load(Ordering::Acquire) as *mut ObjectHeader;
        super::super::js_object_set_field(proto, 0, JSValue::undefined());
        assert_eq!(crate::array::js_array_record_needs_iterator(array), 1);
    }
}

#[test]
fn array_record_rejects_other_brands_and_custom_prototype() {
    unsafe {
        let _stable = crate::gc::GcSuppressScope::new();
        let (array, _, _) = fixture();
        assert_eq!(crate::array::js_array_record_needs_iterator(42.0), 1);
        let text = crate::string::js_string_from_bytes(b"[1]".as_ptr(), 3);
        let lazy = crate::json_tape::with_built_tape(b"[1]", |tape| {
            crate::json_tape::alloc_lazy_array(tape, 0, 1, text)
        })
        .unwrap();
        assert!((*lazy).materialized.is_null());
        assert_eq!(
            crate::array::js_array_record_needs_iterator(js_nanbox_pointer(lazy as i64)),
            1
        );
        assert!(
            (*lazy).materialized.is_null(),
            "eligibility must not materialize another representation"
        );
        let obj = js_nanbox_pointer(js_object_alloc(0, 0) as i64);
        assert_eq!(crate::array::js_array_record_needs_iterator(obj), 1);
        crate::object::js_object_set_prototype_of(array, obj);
        assert_eq!(crate::array::js_array_record_needs_iterator(array), 1);
    }
}

#[test]
fn array_symbol_shape_carrier_is_a_rewritten_gc_child() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    let _verify = crate::gc::knob_overrides::VerifyEvacuationTestGuard::on();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    unsafe {
        let scope = crate::gc::RuntimeHandleScope::new();
        let array =
            scope.root_nanbox_f64(js_nanbox_pointer(crate::array::js_array_alloc(0) as i64));
        let symbol = scope.root_nanbox_f64(js_nanbox_pointer(crate::symbol::well_known_symbol(
            "iterator",
        ) as i64));
        let string = crate::string::js_string_from_bytes(b"owned-edge".as_ptr(), 10);
        let string = scope.root_nanbox_f64(crate::value::js_nanbox_string(string as i64));
        crate::symbol::js_object_set_symbol_property(
            array.get_nanbox_f64(),
            symbol.get_nanbox_f64(),
            string.get_nanbox_f64(),
        );
        let raw = crate::value::js_nanbox_get_pointer(array.get_nanbox_f64()) as usize;
        let before = super::super::shaped_symbols::owner(raw).unwrap() as usize;
        let cycles = crate::gc::copying_minor_cycles();
        crate::gc::gc_collect_minor();
        assert!(crate::gc::copying_minor_cycles() > cycles);
        let raw = crate::value::js_nanbox_get_pointer(array.get_nanbox_f64()) as usize;
        let after = super::super::shaped_symbols::owner(raw).unwrap() as usize;
        assert_ne!(
            before, after,
            "carrier must be rewritten through its owning edge"
        );
        assert_eq!(
            crate::symbol::js_object_get_symbol_property(
                array.get_nanbox_f64(),
                symbol.get_nanbox_f64()
            )
            .to_bits(),
            string.get_nanbox_f64().to_bits()
        );
    }
}

#[test]
fn array_record_next_member_identity_guard() {
    unsafe {
        let _stable = crate::gc::GcSuppressScope::new();
        let (array, _, _) = fixture();
        let original = ARRAY_ITERATOR_PROTOTYPE_PTR.load(Ordering::Acquire) as *mut ObjectHeader;
        let info = super::super::shapes::object_shape_record(original)
            .unwrap()
            .constfn_info(0)
            .unwrap();
        let proto = js_object_alloc(0, 0);
        let other = crate::string::intern_ascii_literal(b"other");
        super::super::global_this::install_proto_method_with_key(
            proto,
            "other",
            info as *const crate::closure::JsFunctionInfo,
            0,
            other,
        );
        set_to_string_tag(proto, "Array Iterator");
        chain_to(
            proto,
            ITERATOR_PROTOTYPE_PTR.load(Ordering::Acquire) as *mut ObjectHeader,
        );
        super::super::shapes::learn_object_constfn_lanes(proto, |slot, _| slot == 0);
        assert!(super::super::shapes::object_shape_record(proto)
            .unwrap()
            .constfn_info(0)
            .is_some());
        ARRAY_ITERATOR_PROTOTYPE_PTR.store(proto as i64, Ordering::Release);
        assert!(!array_record_next_is_builtin());
        assert_eq!(crate::array::js_array_record_needs_iterator(array), 1);
    }
}

#[test]
fn array_record_close_materializes_captured_cursor() {
    unsafe {
        let _stable = crate::gc::GcSuppressScope::new();
        let (array, _, _) = fixture();
        let raw = crate::value::js_nanbox_get_pointer(array) as *mut crate::array::ArrayHeader;
        crate::array::js_array_push_f64(raw, 11.0);
        crate::array::js_array_push_f64(raw, 22.0);
        let iter = crate::array::js_array_record_iterator_at(array, 1.0);
        let method = crate::array::js_iterator_next_method(iter);
        let mut value = 0.0;
        assert_eq!(crate::array::js_iterator_step(iter, method, &mut value), 0);
        assert_eq!(value, 22.0);
    }
}

#[test]
fn array_record_prototype_symbol_identity_guard() {
    unsafe {
        let _stable = crate::gc::GcSuppressScope::new();
        let (array, proto, sym) = fixture();
        let addr = crate::value::js_nanbox_get_pointer(proto) as usize;
        let symbol = crate::value::js_nanbox_get_pointer(sym) as usize;
        let other = crate::symbol::well_known_symbol("toStringTag") as usize;
        let original = crate::symbol::js_object_get_symbol_property(proto, sym);
        let bag = super::super::shaped_symbols::owner(addr).unwrap();
        let slot = super::super::shaped_symbols::position(bag, symbol).unwrap();
        let keys = super::super::object_keys(bag);
        let replacement = crate::array::js_array_alloc(keys.count());
        let mut replacement = replacement;
        for i in 0..keys.count() {
            let key = if i == slot {
                JSValue::from_bits(crate::value::POINTER_TAG | other as u64)
            } else {
                keys.get(i)
            };
            replacement = crate::array::js_array_push(replacement, key);
        }
        // Preserve the original slot and values while changing only the
        // member's identity; an append/delete fixture could miss ConstFn lanes.
        super::super::set_object_keys(bag, super::super::ObjectKeys::owned(replacement));
        super::super::shapes::learn_object_constfn_lanes(bag, |_, bits| bits == original.to_bits());
        assert!(
            super::super::shapes::object_shape_record(bag)
                .unwrap()
                .constfn_info(slot)
                .is_some(),
            "the unrelated member must carry the genuine values body at its actual slot"
        );
        assert_eq!(crate::array::js_array_record_needs_iterator(array), 1);
        super::super::set_object_keys(bag, keys);
    }
}

#[test]
fn array_record_fallback_gets_inherited_iterator_after_prototype_delete() {
    unsafe {
        let _stable = crate::gc::GcSuppressScope::new();
        let (array, proto, sym) = fixture();
        let addr = crate::value::js_nanbox_get_pointer(proto) as usize;
        let symbol = crate::value::js_nanbox_get_pointer(sym) as usize;
        let original = crate::symbol::js_object_get_symbol_property(proto, sym);
        let object = crate::object::builtin_prototype_value("Object");
        let object_addr = crate::value::js_nanbox_get_pointer(object) as usize;
        let previous = super::super::shaped_symbols::get(object_addr, symbol);
        let attrs = super::super::shaped_symbols::entry(object_addr, symbol).unwrap_or(0);
        super::super::shaped_symbols::define(object_addr, symbol, 17.0f64.to_bits(), 0);
        super::super::shaped_symbols::delete(addr, symbol);
        let inherited = crate::symbol::js_object_get_symbol_property(array, sym);
        super::super::shaped_symbols::delete(object_addr, symbol);
        super::super::shaped_symbols::define(addr, symbol, original.to_bits(), 0);
        if let Some(bits) = previous {
            super::super::shaped_symbols::define(object_addr, symbol, bits, attrs);
        }
        assert_eq!(inherited, 17.0, "fallback must perform the full symbol Get");
    }
}

#[test]
fn array_record_next_key_survives_intern_collision() {
    unsafe {
        let _stable = crate::gc::GcSuppressScope::new();
        let (array, _, _) = fixture();
        let proto = ARRAY_ITERATOR_PROTOTYPE_PTR.load(Ordering::Acquire) as *const ObjectHeader;
        let shape = super::super::shapes::object_shape_record(proto).unwrap();
        let old_key =
            *crate::array::array_elements_ptr(shape.keys() as *const crate::array::ArrayHeader);
        let hash = |bytes: &[u8]| {
            bytes.iter().fold(0xcbf29ce484222325u64, |h, b| {
                (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
            })
        };
        let slot = hash(b"next") & 8191;
        let collision = (0..100000)
            .map(|i| format!("array-record-key-{i}"))
            .find(|key| hash(key.as_bytes()) & 8191 == slot)
            .expect("must find an intern-table collision");
        crate::string::intern_ascii_literal(collision.as_bytes());
        let new_key = JSValue::string_ptr(
            crate::string::intern_ascii_literal(b"next") as *mut crate::StringHeader
        )
        .bits();
        assert_ne!(
            old_key, new_key,
            "exercise atom eviction, not the same pointer"
        );
        assert_eq!(crate::array::js_array_record_needs_iterator(array), 0);
    }
}

#[test]
fn array_record_bootstrap_entry_has_current_shape_facts() {
    unsafe {
        // Unlike fixture(), this uses exactly production intrinsic setup,
        // without relearning facts after setup. It catches a vacuous fast path.
        let _ = crate::object::builtin_prototype_value("Array");
        let a = js_nanbox_pointer(crate::array::js_array_alloc(0) as i64);
        assert_eq!(crate::array::js_array_record_needs_iterator(a), 0);
        assert_eq!(crate::array::js_array_record_literal_needs_iterator(), 0);
    }
}

#[test]
fn array_record_entry_repairs_only_its_private_source() {
    unsafe {
        let _stable = crate::gc::GcSuppressScope::new();
        let (alias, _, symbol) = fixture();
        let old = crate::value::js_nanbox_get_pointer(alias) as *mut crate::array::ArrayHeader;
        let live = crate::array::js_array_grow(old, 100);
        let record = crate::array::js_array_record_source(alias);
        assert_eq!(crate::value::js_nanbox_get_pointer(record), live as i64);
        assert_ne!(record.to_bits(), alias.to_bits());
        assert_eq!(crate::array::js_array_record_needs_iterator(record), 0);
        crate::symbol::js_object_set_symbol_property(record, symbol, 33.0);
        assert_eq!(crate::array::js_array_record_needs_iterator(record), 1);
        assert_eq!(crate::array::js_array_record_source(42.0), 42.0);
    }
}
