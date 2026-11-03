//! Static class declarations: computed members named on the holder, ClassBody
//! order across a birth that waits for a name, and the holder namespace.

use super::*;

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

fn method() -> ClassMemberDecl {
    test_method_member(method_body as *const () as usize, 0, false, false, 0)
}

fn literal(cid: u32, name: &str, order: u32) {
    let mut m = method();
    m.definition_order = order;
    test_declare_member(cid, name.as_bytes(), m);
}

/// `Object.getOwnPropertyNames(proto)`.
fn own_names(proto: f64) -> Vec<String> {
    let names = crate::object::js_object_get_own_property_names(proto);
    let arr = crate::value::js_nanbox_get_pointer(names) as *const crate::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    (0..len)
        .map(|i| {
            let v = crate::JSValue::from_bits(crate::array::js_array_get_f64(arr, i).to_bits());
            let mut buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            let bytes = unsafe { crate::string::js_string_key_bytes(v, &mut buf) }.unwrap();
            String::from_utf8_lossy(bytes).into_owned()
        })
        .collect()
}

#[test]
fn computed_member_name_round_trips_wtf8_bytes_without_replacement() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let bytes = [0xed, 0xa0, 0x80]; // WTF-8 encoding of one lone high surrogate.
    let value = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    let value = f64::from_bits(crate::JSValue::string_ptr(value).bits());
    assert_eq!(
        member_name_of_value(value, true),
        MemberName::Str(bytes.to_vec())
    );
    let round_trip = member_name_value(&MemberName::Str(bytes.to_vec()));
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    assert_eq!(
        unsafe {
            crate::string::js_string_key_bytes(
                crate::JSValue::from_bits(round_trip.to_bits()),
                &mut scratch,
            )
        },
        Some(bytes.as_slice())
    );
}

#[test]
fn declaration_method_lookup_matches_exact_wtf8_bytes() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let cid = 0x6F00;
    register(cid, b"Wtf8Lookup");
    let key = [0xed, 0xa0, 0x80];
    test_declare_member(cid, &key, method());

    assert!(class_method_decl_bytes(cid, &key).is_some());
    assert!(class_method_decl_bytes(cid, b"\xef\xbf\xbd").is_none());
}

/// A birth before the definition names a computed member stops there;
/// naming it installs it and the literal members after it, so the prototype
/// keeps ClassBody order.
#[test]
fn computed_member_keeps_classbody_order_across_an_early_birth() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let cid = 0x6F01;
    register(cid, b"Ordered");
    literal(cid, "a", 10);
    test_declare_computed_member(cid, 11, method());
    literal(cid, "b", 12);
    for (name, order) in [("a", 10u32), ("b", 12)] {
        super::super::registration::record_class_string_member_order(
            cid,
            name.as_bytes().to_vec(),
            false,
            order,
        );
    }
    // The prototype is born before the definition evaluates the key.
    let proto = crate::object::class_decl_prototype_value(cid);
    assert_eq!(own_names(proto), ["constructor", "a"]);
    super::super::registration::record_class_string_member_order(cid, b"k".to_vec(), false, 11);
    name_computed_member(cid, 11, MemberName::Str(b"k".to_vec()));
    assert_eq!(own_names(proto), ["constructor", "a", "k", "b"]);
    assert!(crate::object::class_method_slot_target(cid, "b").is_some());
    assert!(crate::object::class_method_slot_target(cid, "k").is_some());
}

/// A symbol-keyed computed member has no string name and does not hold
/// back the members after it.
#[test]
fn symbol_computed_member_does_not_stop_the_string_members() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let cid = 0x6F02;
    register(cid, b"SymbolKeyed");
    literal(cid, "a", 20);
    test_declare_computed_member(cid, 21, method());
    literal(cid, "b", 22);
    let proto = crate::object::class_decl_prototype_value(cid);
    assert_eq!(own_names(proto), ["constructor", "a"]);
    name_computed_member(cid, 21, MemberName::NotAString);
    assert_eq!(own_names(proto), ["constructor", "a", "b"]);
}

/// Re-evaluating a declaration renames its computed member on the holder:
/// one name slot per member however many keys it sees, and the method value
/// kept for the old name is dropped (each name gets its own function object),
/// so the holder does not grow.
#[test]
fn reevaluated_computed_member_keeps_the_holder_bounded() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let cid = 0x6F03;
    register(cid, b"Renamed");
    test_declare_computed_member(cid, 30, method());
    name_computed_member(cid, 30, MemberName::Str(b"k0".to_vec()));
    let holder = crate::object::class_value::class_value_ptr(cid) as usize;
    let mut previous = crate::object::class_prototype_method_value_for_name(cid, "k0").to_bits();
    let baseline = unsafe { crate::closure::props::state_key_count(holder) };
    for i in 1..2000 {
        let name = format!("k{i}");
        name_computed_member(cid, 30, MemberName::Str(name.as_bytes().to_vec()));
        let value = crate::object::class_prototype_method_value_for_name(cid, &name);
        assert_ne!(value.to_bits(), crate::value::TAG_UNDEFINED);
        assert_ne!(
            value.to_bits(),
            previous,
            "a renamed member's value is materialized for its new name"
        );
        previous = value.to_bits();
        // `k0` stays on the decl prototype, which belongs to the evaluation
        // that built it; later evaluations' keys never reach it.
        if i >= 2 {
            assert_eq!(
                crate::object::class_prototype_method_value_for_name(cid, &format!("k{}", i - 1))
                    .to_bits(),
                crate::value::TAG_UNDEFINED,
                "an earlier evaluation's key is not this declaration's member any more"
            );
        }
    }
    let after = unsafe { crate::closure::props::state_key_count(holder) };
    assert!(
        after <= baseline + 2,
        "holder keys grew from {baseline} to {after} over 2000 evaluations"
    );
}

/// The holder's declaration-value keys cannot collide with a member whose
/// name spells another key of the holder's internal namespace.
#[test]
fn holder_namespace_separates_names_values_and_private_statics() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let cid = 0x6F04;
    register(cid, b"Namespace");
    test_declare_computed_member(cid, 40, method());
    // Literal methods spelled like the computed member's name slot and like
    // a private static key.
    literal(cid, "\u{1}k:40", 41);
    literal(cid, "#x", 42);
    name_computed_member(cid, 40, MemberName::Str(b"real".to_vec()));
    let is_function = |v: f64| {
        let js = crate::JSValue::from_bits(v.to_bits());
        js.is_pointer() && crate::closure::is_closure_ptr(js.as_pointer::<u8>() as usize)
    };
    let tricky = crate::object::class_prototype_method_value_for_name(cid, "\u{1}k:40");
    assert!(
        is_function(tricky),
        "the method spelled like a name slot is its own function"
    );
    let private_spelled = crate::object::class_prototype_method_value_for_name(cid, "#x");
    assert!(
        is_function(private_spelled),
        "the method spelled `#x` is its own function"
    );
    // The computed member keeps its own name.
    let decl = class_declaration(cid).unwrap();
    let computed = decl.members().iter().find(|m| m.name.is_null()).unwrap();
    assert_eq!(
        class_member_name(cid, computed),
        MemberName::Str(b"real".to_vec())
    );
    // Each value answers again with the same object.
    assert_eq!(
        crate::object::class_prototype_method_value_for_name(cid, "\u{1}k:40").to_bits(),
        tricky.to_bits()
    );
    assert_eq!(
        crate::object::class_prototype_method_value_for_name(cid, "#x").to_bits(),
        private_spelled.to_bits()
    );
}

/// A declaration's method value is one object per (class, method) once the
/// class has a holder: the holder keeps it.
#[test]
fn method_value_is_kept_on_the_holder() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let cid = 0x6F05;
    register(cid, b"Kept");
    literal(cid, "m", 50);
    let first = crate::object::class_prototype_method_value_for_name(cid, "m");
    assert_ne!(first.to_bits(), crate::value::TAG_UNDEFINED);
    assert!(crate::object::class_value::class_value_if_minted(cid).is_some());
    assert_eq!(
        crate::object::class_prototype_method_value_for_name(cid, "m").to_bits(),
        first.to_bits()
    );
    assert_eq!(
        super::super::state::class_declaration_value(cid, ClassDeclarationValueKind::Method, "m"),
        Some(first.to_bits())
    );
}

/// A class expression's evaluations each name its computed member on their
/// own class object: every evaluation's prototype holds exactly its own key,
/// whatever later evaluations named, and the template's holder is not
/// minted for them.
#[test]
fn class_expression_evaluations_keep_their_own_computed_names() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let cid = 0x6F06;
    register(cid, b"Expr");
    literal(cid, "x", 60);
    test_declare_computed_member(cid, 61, method());
    super::super::registration::record_class_string_member_order(cid, b"x".to_vec(), false, 60);
    let scope = crate::gc::RuntimeHandleScope::new();
    let mut classes = Vec::new();
    for key in ["p", "q"] {
        let class =
            crate::object::field_get_set::js_class_evaluation_object(cid, 6, 0, std::ptr::null())
                as *mut ObjectHeader;
        let class = scope.root_raw_mut_ptr(class);
        let value =
            class.with_mut_ptr::<ObjectHeader, _>(|c| crate::value::js_nanbox_pointer(c as i64));
        super::super::registration::record_class_string_member_order(
            cid,
            key.as_bytes().to_vec(),
            false,
            61,
        );
        unsafe {
            name_evaluation_computed_member(
                value,
                cid,
                61,
                MemberName::Str(key.as_bytes().to_vec()),
                f64::from_bits(crate::value::TAG_UNDEFINED),
            );
        }
        classes.push((key, class));
    }
    assert!(
        crate::object::class_value::class_value_if_minted(cid).is_none(),
        "naming an evaluation's member leaves the template's holder unminted"
    );
    // Both prototypes are built after the second evaluation named its key.
    for (key, class) in &classes {
        let proto = class.with_mut_ptr::<ObjectHeader, _>(|c| unsafe {
            crate::object::field_get_set::class_object_prototype_value(c)
        });
        let names = own_names(f64::from_bits(proto.bits()));
        assert_eq!(names, ["constructor", "x", *key], "evaluation {key}");
    }
}

/// Property keys retain WTF-8 bytes through naming and lazy prototype birth.
#[test]
fn computed_method_names_preserve_surrogate_bytes() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let cid = 0x6F20;
    register(cid, b"RawKeys");
    test_declare_computed_member(cid, 90, method());
    let bytes = vec![0xED, 0xA0, 0x80];
    name_computed_member(cid, 90, MemberName::Str(bytes.clone()));
    assert!(class_method_decl_bytes(cid, &bytes).is_some());
    assert!(class_method_decl_bytes(cid, b"").is_none());
    assert!(class_method_decl(cid, "\u{FFFD}").is_none());
    let prototype = crate::object::class_decl_prototype_value(cid);
    let object = crate::JSValue::from_bits(prototype.to_bits()).as_pointer::<crate::ObjectHeader>();
    unsafe {
        assert!(crate::object::own_property_get_by_bytes(object, &bytes, prototype).is_some());
        assert!(crate::object::own_property_get_by_bytes(object, b"", prototype).is_none());
    }
}

#[test]
fn native_names_carry_the_utf8_fact_and_unproven_wtf8_names_name_no_member() {
    let code = method_body as *const () as *const u8;
    for m in [
        ClassMemberDecl::native_method("size", code, 0),
        ClassMemberDecl::native_accessor("at", code, false),
        ClassMemberDecl::native_accessor("at", code, true),
    ] {
        assert_ne!(m.flags & CLASS_MEMBER_NAME_UTF8, 0, "{m:?}");
        assert!(m.literal_name().is_some_and(|n| n == "size" || n == "at"));
    }
    // A lone surrogate (WTF-8) without the fact is validated and names no member.
    static LONE: [u8; 3] = [0xED, 0xA0, 0x80];
    let mut w = method();
    w.name = LONE.as_ptr();
    w.name_len = LONE.len() as u32;
    assert_eq!(w.flags & CLASS_MEMBER_NAME_UTF8, 0);
    assert_eq!(w.literal_name(), None);
}

fn declared(cid: u32) -> Vec<(String, u32)> {
    class_declared_members(cid)
        .0
        .into_iter()
        .map(|m| (String::from_utf8(m.name).unwrap(), m.at.method))
        .collect()
}

#[test]
fn declared_members_find_the_last_declaration_of_each_name_once() {
    // Bytes path: the test builder copies each name to its own pointer.
    let cid = 0x7E57_0001;
    register(cid, b"Dup");
    literal(cid, "a", 1);
    literal(cid, "b", 2);
    literal(cid, "a", 3);
    assert_eq!(declared(cid), [("a".into(), 2), ("b".into(), 1)]);
    // The index the walk found is the member the by-name search finds.
    for (name, index) in declared(cid) {
        let found = class_method_decl_bytes(cid, name.as_bytes()).unwrap();
        let at = class_method_decl_at(cid, index).unwrap();
        assert!(std::ptr::eq(found, at), "{name}");
    }
}

#[test]
fn a_pooled_declaration_compares_name_pointers_not_bytes() {
    static A: &[u8] = b"alpha";
    static A_COPY: &[u8] = b"alpha";
    static B: &[u8] = b"beta";
    // The compiler's contract: equal names share a pointer.
    let pooled = 0x7E57_0002;
    test_declare_pointer_named(pooled, &[A, B, A], true);
    assert_eq!(declared(pooled), [("alpha".into(), 2), ("beta".into(), 1)]);
    // The runtime trusts the flag: a pooled declaration naming one string by
    // two pointers lists it twice. This is what fails if the compare reads
    // bytes.
    let split = 0x7E57_0003;
    let (p, q) = (A.as_ptr(), A_COPY.as_ptr());
    if p != q {
        test_declare_pointer_named(split, &[A, A_COPY], true);
        assert_eq!(declared(split), [("alpha".into(), 0), ("alpha".into(), 1)]);
    }
    // Without the flag the same members dedupe by bytes.
    let unpooled = 0x7E57_0004;
    test_declare_pointer_named(unpooled, &[A, A_COPY, B], false);
    assert_eq!(
        declared(unpooled),
        [("alpha".into(), 1), ("beta".into(), 2)]
    );
}

#[test]
fn a_computed_member_joins_a_literal_of_the_same_name_by_bytes() {
    let cid = 0x7E57_0005;
    register(cid, b"Mixed");
    literal(cid, "k", 1);
    test_declare_computed_member(cid, 2, method());
    name_computed_member(cid, 2, MemberName::Str(b"k".to_vec()));
    // The computed member is the later declaration of the name `k`.
    assert_eq!(declared(cid), [("k".into(), 1)]);
}
