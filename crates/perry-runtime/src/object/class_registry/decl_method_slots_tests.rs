//! A declared class's prototype holds a function object of each method's own
//! body, and the prototype's shape names that body in the slot's lane.

use super::*;

extern "C" fn entry_body(
    _: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    0.0
}

extern "C" fn method_body(_this: f64) -> f64 {
    0.0
}

fn register(cid: u32, name: &[u8]) {
    {
        let mut guard = crate::object::REGISTERED_CLASS_IDS.write().unwrap();
        guard
            .get_or_insert_with(crate::fast_hash::new_ptr_hash_set)
            .insert(cid);
    }
    unsafe {
        crate::object::js_register_class_name(cid, name.as_ptr(), name.len() as u32);
        crate::object::js_register_class_length(cid, 0);
    }
}

/// Module init's registration of a compiled method: one call, carrying the
/// method's closure-convention entry when it has one.
unsafe fn register_method(cid: u32, name: &[u8], entry: Option<usize>) {
    super::js_register_class_method_with_entry(
        cid as i64,
        name.as_ptr(),
        name.len() as i64,
        method_body as *const () as usize as i64,
        0,
        0,
        0,
        entry.unwrap_or(0) as i64,
    );
}

/// The inline slot of `proto` holding exactly `bits`.
unsafe fn slot_holding(proto: *const ObjectHeader, bits: u64) -> u32 {
    let fields = (proto as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
    (0..crate::object::object_live_slot_count(proto))
        .find(|&i| std::ptr::read(fields.add(i as usize)) == bits)
        .expect("the method value sits in an inline slot of the prototype")
}

#[test]
fn decl_prototype_method_slot_is_a_constfn_lane_of_its_body() {
    let cid = 0x6E31;
    register(cid, b"Decl");
    let info = crate::fn_info!(entry_body, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE))
        as *const crate::closure::JsFunctionInfo as usize;
    unsafe { register_method(cid, b"m", Some(info)) };

    let proto = class_decl_prototype_value(cid);
    assert!(
        crate::value::JSValue::from_bits(proto.to_bits()).is_pointer(),
        "the prototype object"
    );
    let proto = crate::value::JSValue::from_bits(proto.to_bits()).as_pointer::<ObjectHeader>();
    let m = class_object_own_field_bytes(proto, b"m")
        .expect("m is an own data property of the prototype")
        .to_bits();
    let closure = (m & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
    unsafe {
        assert!(crate::closure::is_closure_ptr(closure as usize));
        assert_eq!(
            (*closure).info as usize,
            info,
            "m runs its own body's entry"
        );
        let slot = slot_holding(proto, m);
        let record = crate::object::shapes::shape_record_by_id(
            crate::object::shapes::object_shape_stamp(proto),
        )
        .expect("the prototype is shaped");
        assert_eq!(
            record.constfn_info(slot),
            Some(info as u64),
            "the prototype's shape names m's body in its slot"
        );
        assert_eq!(
            crate::object::class_method_entry_source_func_ptr(closure),
            Some(method_body as *const () as usize),
            "its source is the method body's"
        );
    }
    // `c.m`, `C.prototype.m` and the slot are one function object, and it
    // is named when it is built (module init registers no name for it).
    assert_eq!(
        crate::object::class_prototype_method_value_for_name(cid, "m").to_bits(),
        m
    );
    assert_eq!(
        crate::builtins::function_name_for_ptr(unsafe { (*closure).code() } as usize).as_deref(),
        Some("m")
    );
}

#[test]
fn a_method_without_an_entry_keeps_its_body_on_the_function_object() {
    let cid = 0x6E32;
    register(cid, b"Old");
    unsafe { register_method(cid, b"m", None) };
    let m = crate::object::class_prototype_method_value_for_name(cid, "m").to_bits();
    let closure = (m & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
    unsafe {
        assert!(crate::closure::is_closure_ptr(closure as usize));
        assert_ne!(
            (*closure).code(),
            crate::closure::BOUND_METHOD_FUNC_PTR,
            "native declarations retain their body without redispatch"
        );
        assert_eq!(
            crate::object::class_method_entry_source_func_ptr(closure),
            Some(method_body as *const () as usize)
        );
    }
}

#[test]
fn ordinary_four_capture_closure_has_no_declaration_body() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let cid = 0x6E33;
    register(cid, b"Capture");
    let home = crate::object::class_value::class_value(cid);
    let f = crate::closure::js_closure_alloc(crate::fn_info!(entry_body, 0), 4);
    unsafe {
        crate::closure::closure_install_boxed_captures(
            f,
            &[
                home.to_bits(),
                crate::value::INT32_TAG
                    | crate::object::native_module::CLASS_PROTOTYPE_REF_FLAG
                    | 123,
                crate::value::INT32_TAG
                    | crate::object::native_module::CLASS_PROTOTYPE_REF_FLAG
                    | 456,
                0.0f64.to_bits(),
            ],
        );
        assert_eq!(
            crate::object::native_module::class_method_value_target(
                crate::value::js_nanbox_pointer(f as i64).to_bits()
            ),
            None
        );
    }
}

extern "C" fn private_getter(_this: f64) -> f64 {
    42.0
}

#[test]
fn s7b_private_accessor_reads_its_materialized_pair() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let cid = 0x6E34;
    register(cid, b"PrivatePair");
    unsafe {
        super::js_register_class_getter(
            cid as i64,
            b"#x".as_ptr(),
            2,
            private_getter as *const () as usize as i64,
        );
        // The declaration installed the pair on its function's traced holder.
        // Removing materialization input cannot change the private read.
        CLASS_VTABLE_REGISTRY.write().unwrap().as_mut().unwrap().remove(&cid);
        for _ in 0..2 {
            assert_eq!(
                super::class_private_instance_getter_value(cid, "#x", 0.0),
                Some(42.0)
            );
        }
    }
}

#[test]
fn s7b_private_accessor_pair_cannot_collide_with_method_names() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let cid = 0x6E36;
    register(cid, b"PrivateNamespace");
    // A computed public method can spell an internal-looking prefix or the
    // private member's spelling. Both method entries must remain distinct
    // from the accessor pair in the existing materialized-value store.
    let names = ["\0private-accessor:#x", "#x"];
    let mut methods = Vec::new();
    for name in names {
        unsafe { register_method(cid, name.as_bytes(), None) };
        methods.push(crate::object::class_prototype_method_value_for_name(cid, name).to_bits());
    }
    unsafe {
        super::js_register_class_getter(
            cid as i64,
            b"#x".as_ptr(),
            2,
            private_getter as *const () as usize as i64,
        );
        for _ in 0..2 {
            assert_eq!(
                super::class_private_instance_getter_value(cid, "#x", 0.0),
                Some(42.0)
            );
        }
    }
    for (name, method) in names.into_iter().zip(methods) {
        assert_eq!(
            crate::object::class_prototype_method_value_for_name(cid, name).to_bits(),
            method
        );
    }
}

#[test]
fn s7b_detached_instance_does_not_recover_a_declaration_method() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let cid = 0x6E35;
    register(cid, b"Detached");
    unsafe { register_method(cid, b"m", None) };
    class_decl_prototype_value(cid);
    let obj = crate::object::js_object_alloc(cid, 0);
    let receiver = crate::value::js_nanbox_pointer(obj as i64);
    // Both an empty recorded chain and null must prevent a canonical
    // declaration fallback in the method-as-value path.
    let empty = crate::object::js_object_alloc(0, 0);
    for prototype in [
        crate::value::js_nanbox_pointer(empty as i64).to_bits(),
        crate::value::TAG_NULL,
    ] {
        crate::object::prototype_chain::object_set_user_prototype(obj as usize, prototype);
        assert_eq!(
            crate::object::js_class_method_bind(receiver, b"m".as_ptr(), 1).to_bits(),
            crate::value::TAG_UNDEFINED
        );
    }
}

#[test]
fn s7b_non_property_owner_walk_stops_at_an_exotic_holder() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let cid = 0x6E37;
    register(cid, b"ArrayHop");
    let prototype = crate::object::class_decl_prototype_value(cid);
    let array = crate::array::js_array_alloc(0);
    crate::object::prototype_chain::object_set_user_prototype(
        (prototype.to_bits() & crate::value::POINTER_MASK) as usize,
        crate::value::js_nanbox_pointer(array as i64).to_bits(),
    );
    assert_eq!(
        crate::object::class_method_slot_target(cid, "#missing"),
        None
    );
}

#[test]
fn s7b_literal_symbol_alias_method_cannot_be_recovered_after_delete() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let cid = 0x6E38;
    let name = b"@@iterator";
    register(cid, b"LiteralAlias");
    unsafe {
        super::js_register_class_string_member_order(
            cid as i64,
            name.as_ptr(),
            name.len() as i64,
            0,
            0,
        );
        register_method(cid, name, None);
    }
    let prototype = crate::object::class_decl_prototype_value(cid);
    assert!(crate::object::class_method_slot_target(cid, "@@iterator").is_some());
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let holder =
        (prototype.to_bits() & crate::value::POINTER_MASK) as *mut crate::object::ObjectHeader;
    assert_eq!(crate::object::js_object_delete_field(holder, key), 1);
    assert_eq!(
        crate::object::class_method_slot_target(cid, "@@iterator"),
        None
    );
}

#[test]
fn s7b_evaluation_method_materialization_does_not_build_a_template_holder() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let cid = 0x6E3A;
    register(cid, b"ComputedBody");
    unsafe { register_method(cid, b"m", None) };
    let class = crate::object::js_class_evaluation_object(cid, 6, 0, std::ptr::null_mut());
    let brand = crate::value::js_nanbox_pointer(class as i64);
    assert!(crate::object::class_holder_prototype(cid).is_null());
    let method = crate::object::class_evaluation_method_value_for_name(cid, "m", brand);
    assert_ne!(method.to_bits(), crate::value::TAG_UNDEFINED);
    assert!(
        crate::object::class_holder_prototype(cid).is_null(),
        "materializing an evaluation method must not re-evaluate its superclass prototype"
    );
}

#[test]
fn s7b_anon_shape_identity_has_no_class_surface() {
    // An object literal's id is registered for `typeof` and may carry
    // literal methods, but its [[Prototype]] is Object.prototype: it must
    // never build a declaration holder or publish a CLASS word.
    let cid = 0x6E3C;
    unsafe {
        crate::object::js_register_class_id(cid);
        crate::object::js_register_anon_shape_class_id(cid);
        register_method(cid, b"f", None);
    }
    assert!(!super::state::class_identity_has_surface(cid));
    assert_eq!(
        crate::object::shapes::identity_prototype_word(
            crate::object::shapes::PROTO_ID_CLASS | u64::from(cid)
        ),
        0,
        "a literal's method registration must not publish an unbuilt class word"
    );
    assert_eq!(
        class_decl_prototype_value(cid).to_bits(),
        crate::value::TAG_UNDEFINED,
        "a literal's id must not build a declaration holder"
    );
    assert!(!crate::object::class_has_own_method(cid, "missing"));
    assert!(!crate::object::class_instance_has_method(cid, "missing"));
    assert_eq!(class_method_slot_owner(cid, "missing"), None);
    assert_eq!(class_method_slot_value(cid, "missing"), None);
    assert_eq!(crate::object::class_holder_word(cid), 0);
    // A declared class keeps its surface; the anon mark alone decides nothing
    // for a named collision.
    let declared = 0x6E3D;
    register(declared, b"Declared");
    unsafe { crate::object::js_register_anon_shape_class_id(declared) };
    assert!(super::state::class_identity_has_surface(declared));
}

#[test]
fn s7b_empty_anonymous_declaration_publishes_surface_and_inherits_methods() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let parent = 0x6E3E;
    let child = 0x6E3F;
    register(parent, b"Parent");
    unsafe { register_method(parent, b"m", None) };
    register(child, b"");
    crate::object::js_register_class_parent(child, parent);
    assert_eq!(
        crate::object::class_holder_word(child),
        crate::value::TAG_UNDEFINED
    );
    assert!(crate::object::class_holder_prototype(child).is_null());
    assert!(!crate::object::class_has_own_method(child, "m"));
    assert!(crate::object::class_instance_has_method(child, "m"));
    assert_eq!(class_method_slot_owner(child, "m"), Some(parent));
    assert!(class_method_slot_value(child, "m").is_some());
    assert!(!crate::object::class_holder_prototype(child).is_null());
}
