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
fn a_method_without_an_entry_keeps_the_name_trampoline() {
    let cid = 0x6E32;
    register(cid, b"Old");
    unsafe { register_method(cid, b"m", None) };
    let m = crate::object::class_prototype_method_value_for_name(cid, "m").to_bits();
    let closure = (m & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
    unsafe {
        assert!(crate::closure::is_closure_ptr(closure as usize));
        assert_eq!(
            (*closure).code(),
            crate::closure::BOUND_METHOD_FUNC_PTR,
            "no entry: the trampoline"
        );
        assert_eq!(
            crate::object::class_method_entry_source_func_ptr(closure),
            None
        );
    }
}
