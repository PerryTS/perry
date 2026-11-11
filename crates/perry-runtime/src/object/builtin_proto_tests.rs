use super::*;

#[test]
fn shared_intrinsic_families_keep_their_abi_slots() {
    // Both runtime and compiler use perry-abi::BuiltinProto.
    let expected: [(BuiltinProto, u8); FAMILIES] = [
        (BuiltinProto::Object, 0),
        (BuiltinProto::Function, 1),
        (BuiltinProto::Array, 2),
        (BuiltinProto::String, 3),
        (BuiltinProto::Number, 4),
        (BuiltinProto::Boolean, 5),
        (BuiltinProto::Symbol, 6),
        (BuiltinProto::BigInt, 7),
        (BuiltinProto::Map, 8),
        (BuiltinProto::Set, 9),
        (BuiltinProto::Date, 10),
        (BuiltinProto::RegExp, 11),
        (BuiltinProto::Promise, 12),
        (BuiltinProto::Error, 13),
        (BuiltinProto::WeakMap, 14),
        (BuiltinProto::WeakSet, 15),
        (BuiltinProto::Buffer, 16),
    ];
    for (family, byte) in expected {
        assert_eq!(family as u8, byte);
        assert_eq!(BuiltinProto::ALL[byte as usize].0, family);
    }
}

/// A stand-in intrinsic of `family` for this test thread: an ordinary,
/// shaped object, sealed and published in the agent slot the fact reads.
unsafe fn sealed(family: BuiltinProto) -> *mut ObjectHeader {
    let proto = crate::object::js_object_alloc(0, 2);
    let key = crate::string::js_string_from_bytes(b"k".as_ptr(), 1);
    crate::object::js_object_set_field_by_name(proto, key, 1.0);
    assert!(
        crate::object::shapes::is_site_matchable_layout_word((*proto).parent_class_id),
        "premise: an ordinary layout word"
    );
    seal_object(proto, family);
    crate::agent_ptrs::publish_intrinsic_proto(family as usize, proto);
    proto
}

/// `sealed` with an own `constructor` in the image, as every intrinsic
/// prototype has one: the program's later write of it is the exempt REPLACE
/// (DEC 250 narrowed lock), not an add the lock refuses.
unsafe fn sealed_with_constructor(family: BuiltinProto) -> *mut ObjectHeader {
    let proto = crate::object::js_object_alloc(0, 2);
    let key = crate::string::js_string_from_bytes(b"k".as_ptr(), 1);
    crate::object::js_object_set_field_by_name(proto, key, 1.0);
    let ctor = crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
    crate::object::js_object_set_field_by_name(proto, ctor, 0.5);
    seal_object(proto, family);
    crate::agent_ptrs::publish_intrinsic_proto(family as usize, proto);
    proto
}

/// Add own key `name` to `obj` through the program's store funnel.
unsafe fn add_key(obj: *mut ObjectHeader, name: &[u8], value: f64) {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::object::js_object_set_field_by_name(obj, key, value);
}

unsafe fn meta_of(obj: *mut ObjectHeader) -> *mut super::super::ObjectMeta {
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj));
    assert!(!meta.is_null(), "premise: a sealed object has a meta record");
    meta
}

#[test]
fn a_sealed_object_withdraws_its_family_on_a_program_write_and_never_reopens() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        assert!(pristine(BuiltinProto::Map), "starts pristine");
        let proto = sealed(BuiltinProto::Map);
        let set = sealed(BuiltinProto::Set);
        assert!(pristine(BuiltinProto::Map), "sealing is not a write");
        // DEC 250: the program may ADD a member (its image member `k` at
        // slot 0 it may not write); the add withdraws the fact.
        add_key(proto, b"p", 1.0);
        assert!(!pristine(BuiltinProto::Map));
        assert!(pristine(BuiltinProto::Set), "other families stay pristine");
        assert!(super::super::layout_desc::word_is_shadowed(
            (*proto).parent_class_id
        ));
        assert!(!super::super::layout_desc::word_is_shadowed(
            (*set).parent_class_id
        ));
        // A store into the ADDED member's slot is admitted and keeps the
        // fact withdrawn (set-only).
        crate::object::store_object_field_slot(proto, 1, 8.0f64.to_bits());
        assert!(!pristine(BuiltinProto::Map), "set-only");
    }
}

/// DEC 250: the decision every mutation funnel of a sealed intrinsic reaches
/// (`note_sealed_write` throws on `Some`): a value store into an image slot,
/// a delete or redefinition of an image key and a relink are violations; an
/// add, a write of a program-added member and a delete of an absent key are
/// not; the image stays locked after the program added a member.
#[test]
fn a_locked_intrinsic_refuses_its_image_members_and_admits_additions() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        // The image gives the stand-in exactly one member, `k`, at slot 0.
        let proto = sealed(BuiltinProto::Map);
        let meta = meta_of(proto);
        assert!(locked_family(BuiltinProto::Map as usize));
        assert_eq!(
            locked_violation(proto, meta, Mutation::Replace(0)),
            Some(Violation::Replace(".k".into())),
            "a value store into an image slot"
        );
        assert_eq!(
            locked_violation(proto, meta, Mutation::Delete(Key::Bytes(b"k"))),
            Some(Violation::Delete(".k".into()))
        );
        assert_eq!(
            locked_violation(proto, meta, Mutation::Redefine(Key::Bytes(b"k"))),
            Some(Violation::Redefine(".k".into()))
        );
        assert_eq!(
            locked_violation(proto, meta, Mutation::Relink),
            Some(Violation::Relink)
        );
        assert_eq!(
            locked_violation(proto, meta, Mutation::Add(Key::Bytes(b"p"))),
            None,
            "a key-add transition is never an image member"
        );
        assert_eq!(
            locked_violation(proto, meta, Mutation::Delete(Key::Bytes(b"absent"))),
            None
        );
        assert_eq!(
            locked_violation(proto, meta, Mutation::Replace(1)),
            None,
            "slot 1 is not in the image"
        );
        // The program adds `p` (slot 1): re-setting or deleting it is fine,
        // the image member is still locked.
        add_key(proto, b"p", 1.0);
        assert_eq!(image_key_count(meta, own_keys(proto)), 1, "recorded at the first write");
        assert_eq!(locked_violation(proto, meta, Mutation::Replace(1)), None);
        assert_eq!(
            locked_violation(proto, meta, Mutation::Delete(Key::Bytes(b"p"))),
            None
        );
        assert_eq!(
            locked_violation(proto, meta, Mutation::Replace(0)),
            Some(Violation::Replace(".k".into()))
        );
        // A key the funnels hold as a value or as a header resolves the same.
        let k = crate::string::js_string_from_bytes(b"k".as_ptr(), 1);
        assert_eq!(
            locked_violation(proto, meta, Mutation::Delete(Key::Str(k))),
            Some(Violation::Delete(".k".into()))
        );
        assert_eq!(
            locked_violation(
                proto,
                meta,
                Mutation::Delete(Key::Value(crate::value::js_nanbox_string(k as i64)))
            ),
            Some(Violation::Delete(".k".into()))
        );
        assert_eq!(
            locked_violation(proto, meta, Mutation::Delete(Key::Value(0.0))),
            None,
            "an index key is never an image member"
        );
        // A post-mutation report decides nothing.
        assert_eq!(locked_violation(proto, meta, Mutation::Publication), None);
    }
}

/// DEC 250's lock set (owner ruling 2026-11-03) is a function of the sealed
/// family: the builtin PROTOTYPE families only. Namespace holders and
/// constructor statics (Math.random, Date.now), `console`, the realm global
/// and runtime prototypes stay patchable.
#[test]
fn the_lock_set_is_the_builtin_prototype_families() {
    use super::super::builtin_namespace::{CONSOLE_FAMILY, GLOBAL_FAMILY};
    assert!(!locked_family(CONSOLE_FAMILY));
    assert!(!locked_family(GLOBAL_FAMILY));
    assert!(!locked_family(RUNTIME_FAMILY));
    assert!(locked_family(ITERATOR_FAMILY));
    assert!(locked_family(ARRAY_ITERATOR_FAMILY));
    assert!(locked_family(GENERATOR_FAMILY));
    assert!(locked_family(ASYNC_GENERATOR_FAMILY));
    assert!(!add_locked(GENERATOR_FAMILY));
    assert!(!add_locked(ASYNC_GENERATOR_FAMILY));
    for family in [
        MAP_ITERATOR_FAMILY,
        SET_ITERATOR_FAMILY,
        STRING_ITERATOR_FAMILY,
        REGEXP_STRING_ITERATOR_FAMILY,
    ] {
        assert!(locked_family(family), "{family}");
        assert!(!add_locked(family), "{family}");
    }
    // DEC 250 locks the ECMA intrinsics; Buffer.prototype stays patchable.
    for (family, _) in BuiltinProto::ALL {
        assert_eq!(
            locked_family(family as usize),
            family.realm_intrinsic(),
            "{family:?}"
        );
    }
    for name in ["Math", "JSON", "Date", "Object", "Array", "Reflect"] {
        let family = perry_abi::builtin_namespace_family(name).unwrap();
        assert!(!locked_family(family), "{name}");
    }
}

/// The one program write `%Object.prototype%` still admits (DEC 250 ruling
/// 11-03: `constructor` stays patchable) withdraws every family that inherits
/// it; the runtime's own definition of the member withdraws none.
#[test]
fn object_prototype_withdraws_every_family() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let protos: Vec<_> = BuiltinProto::REALM.iter().map(|(f, _)| sealed_with_constructor(*f)).collect();
        // A definition that adds no key (a value refresh) withdraws nothing; a
        // definition ADD withdraws (a_definition_add_on_an_add_locked_prototype_…).
        super::super::own_override::as_builtin_definition(|| add_key(protos[0], b"k", 1.5));
        for (family, _) in BuiltinProto::REALM {
            assert!(pristine(family), "{family:?}: a definition that adds nothing withdraws nothing");
        }
        add_key(protos[0], b"constructor", 2.0);
        for (family, _) in BuiltinProto::REALM {
            assert!(!pristine(family), "{family:?} inherits Object.prototype");
        }
    }
}

#[test]
fn an_unsealed_object_never_touches_the_fact() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let _ = sealed(BuiltinProto::Date);
        let obj = crate::object::js_object_alloc(0, 2);
        crate::object::store_object_field_slot(obj, 0, 1.0f64.to_bits());
        crate::object::store_object_field_slot(obj, 1, 2.0f64.to_bits());
        for (family, _) in BuiltinProto::ALL {
            assert!(pristine(family), "{family:?}");
        }
    }
}

#[test]
fn a_builtin_definition_does_not_withdraw_the_fact() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let proto = sealed(BuiltinProto::Date);
        crate::object::own_override::as_builtin_definition(|| {
            // The runtime's own write of an image slot: admitted (DEC 250
            // locks the program, not the population) and not a withdrawal.
            crate::object::store_object_field_slot(proto, 0, 3.0f64.to_bits());
        });
        assert!(pristine(BuiltinProto::Date));
        assert_eq!(
            locked_violation(proto, meta_of(proto), Mutation::Replace(0)),
            Some(Violation::Replace(".k".into())),
            "the image stays locked for the program"
        );
    }
}

extern "C" fn function_receiver_fixture(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    1.0
}

/// The replacement returns its own receiver's bits, so the assertion can see
/// both that it ran and which `this` it received.
extern "C" fn replaced_prototype_method_fixture(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    this.as_f64()
}

/// #9202 (`globals/function-tostring-override`): the generic `[[Get]]` the
/// failed-`Pristine` redirect performs cannot see a FUNCTION receiver's
/// method surface — `js_object_get_field_by_name_f64`'s closure arms report
/// `toString`/`valueOf`/`isPrototypeOf` absent because dedicated dispatch owns
/// them — so a program that replaces a method on the function's prototype made
/// `fn.method()` throw `TypeError: value is not a function` instead of calling
/// the replacement. `spec_get`'s function-receiver fallback must resolve the
/// name off the receiver's real `[[Prototype]]`.
#[test]
fn a_replaced_prototype_method_is_called_on_a_function_receiver() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let scope = crate::gc::RuntimeHandleScope::new();
    unsafe {
        let receiver = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::closure::js_closure_alloc(crate::fn_info!(function_receiver_fixture, 0), 0)
                as i64,
        ));
        let proto = crate::object::js_object_alloc(0, 1);
        let replacement = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::closure::js_closure_alloc(
                crate::fn_info!(replaced_prototype_method_fixture, 0),
                0,
            ) as i64,
        ));
        let key = crate::string::js_string_from_bytes(b"toString".as_ptr(), 8);
        crate::object::js_object_set_field_by_name(proto, key, replacement.get_nanbox_f64());
        crate::object::js_object_set_prototype_of(
            receiver.get_nanbox_f64(),
            crate::value::js_nanbox_pointer(proto as i64),
        );

        // Premise: the receiver IS a function, and the plain object it
        // inherits from really holds the replacement — a vacuous test below
        // would otherwise pass on a missing method.
        assert!(
            crate::closure::is_closure_ptr(crate::value::js_nanbox_get_pointer(
                receiver.get_nanbox_f64()
            ) as usize),
            "premise: the receiver must be a function"
        );
        assert_eq!(
            crate::object::js_object_get_field_by_name_f64(
                proto as *const crate::object::ObjectHeader,
                key
            )
            .to_bits(),
            replacement.get_nanbox_u64(),
            "premise: the prototype owns the replacement"
        );

        // The `Get` the redirect performs must answer with the replacement.
        let selected =
            crate::object::method_call::spec_get_value(receiver.get_nanbox_f64(), b"toString");
        assert_eq!(
            selected.to_bits(),
            replacement.get_nanbox_u64(),
            "the redirect's Get must see the inherited method on a function"
        );

        // And the redirect must call it with the function as `this`.
        let called = redirect_call(receiver.get_nanbox_f64(), b"toString", std::ptr::null(), 0);
        assert_eq!(
            called.map(|value| value.to_bits()),
            Some(receiver.get_nanbox_u64()),
            "the replacement must run with the function as `this`"
        );
    }
}

/// A detached function (`Object.setPrototypeOf(fn, null)`) has no inherited
/// method surface, so the fallback must find nothing there and leave the
/// redirect's non-callable verdict intact.
#[test]
fn a_detached_function_has_no_inherited_method_for_the_redirect() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let scope = crate::gc::RuntimeHandleScope::new();
    unsafe {
        let receiver = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::closure::js_closure_alloc(crate::fn_info!(function_receiver_fixture, 0), 0)
                as i64,
        ));
        crate::object::js_object_set_prototype_of(
            receiver.get_nanbox_f64(),
            f64::from_bits(crate::value::TAG_NULL),
        );
        assert_eq!(
            crate::object::method_call::spec_get_value(receiver.get_nanbox_f64(), b"toString")
                .to_bits(),
            crate::value::TAG_UNDEFINED,
            "a detached function inherits no method"
        );
    }
}

/// DEC 250 (i): every key that is not own is refused on the four add-locked
/// prototypes, whichever funnel reports it: a key-add transition, a
/// definition of an absent key, or a stamp that only carries the published
/// layout word. Elsewhere an add stays admitted.
#[test]
fn the_add_locked_prototypes_refuse_every_new_key() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        for family in [BuiltinProto::Object, BuiltinProto::Function] {
            let proto = sealed(family);
            let meta = meta_of(proto);
            assert_eq!(
                locked_violation(proto, meta, Mutation::Add(Key::Bytes(b"p"))),
                Some(Violation::Add(".p".into())),
                "{family:?}"
            );
            assert_eq!(
                locked_violation(proto, meta, Mutation::Redefine(Key::Bytes(b"absent"))),
                Some(Violation::Add(".absent".into()))
            );
            assert_eq!(
                locked_violation(proto, meta, Mutation::Redefine(Key::Bytes(b"k"))),
                Some(Violation::Redefine(".k".into())),
                "an own image key is a redefinition, not an add"
            );
            // The same transition chain on an ordinary object names the word
            // a key-add stamp of `p` would publish.
            let twin = crate::object::js_object_alloc(0, 2);
            add_key(twin, b"k", 1.0);
            add_key(twin, b"p", 2.0);
            assert_eq!(
                locked_violation(proto, meta, Mutation::Stamp((*twin).parent_class_id)),
                Some(Violation::Add(".p".into())),
                "a stamp that appends a key is an add"
            );
            assert_eq!(
                locked_violation(proto, meta, Mutation::Stamp((*proto).parent_class_id)),
                None,
                "a stamp that appends nothing decides nothing"
            );
        }
        let map = sealed(BuiltinProto::Map);
        let meta = meta_of(map);
        assert_eq!(locked_violation(map, meta, Mutation::Add(Key::Bytes(b"p"))), None);
        assert_eq!(
            locked_violation(map, meta, Mutation::Redefine(Key::Bytes(b"absent"))),
            None
        );
    }
}

/// DEC 250 (i): the iterator prototypes are sealed by their builder, carry
/// their own families, and refuse replacement and adds alike.
#[test]
fn the_iterator_prototypes_are_sealed_and_add_locked() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        super::super::iterator_prototypes::ensure_iterator_prototypes();
        let array_iterator =
            super::super::iterator_prototypes::array_iterator_prototype_addr() as *mut ObjectHeader;
        assert!(!array_iterator.is_null());
        let meta = meta_of(array_iterator);
        assert_ne!((*meta).flags & super::super::meta_flags::OBJECT_META_FLAG_SEALED_INTRINSIC, 0);
        assert_eq!(sealed_family((*meta).flags), ARRAY_ITERATOR_FAMILY);
        assert_eq!(
            locked_violation(array_iterator, meta, Mutation::Add(Key::Bytes(b"foo"))),
            Some(Violation::Add(".foo".into()))
        );
        assert_eq!(locked_object_name(ITERATOR_FAMILY), "Iterator.prototype");
        assert_eq!(locked_object_name(ARRAY_ITERATOR_FAMILY), "%ArrayIteratorPrototype%");
        // A stand-in sealed into the family refuses an image member too.
        let proto = crate::object::js_object_alloc(0, 1);
        add_key(proto, b"k", 1.0);
        seal_object_family(proto, ITERATOR_FAMILY);
        let meta = meta_of(proto);
        assert_eq!(
            locked_violation(proto, meta, Mutation::Replace(0)),
            Some(Violation::Replace(".k".into()))
        );
    }
}

/// DEC 250 (i): an index add on `%Array.prototype%` / `%Object.prototype%`
/// is refused for the program only; the runtime's definitions pass.
#[test]
fn an_index_add_is_refused_outside_builtin_definition() {
    super::super::own_override::as_builtin_definition(|| {
        refuse_index_add(BuiltinProto::Array);
        refuse_index_add(BuiltinProto::Object);
    });
    assert!(add_locked(BuiltinProto::Object as usize));
    assert!(!add_locked(BuiltinProto::Array as usize), "named adds stay admitted");
}

/// The message of a thrown error, read out of its `ErrorHeader`.
unsafe fn thrown_message(exception: f64) -> String {
    let err = crate::value::js_nanbox_get_pointer(exception) as *const crate::error::ErrorHeader;
    assert!(!err.is_null(), "a thrown value must be an error pointer");
    let message = (*err).message;
    if message.is_null() {
        return String::new();
    }
    let data = crate::string::string_data(message);
    String::from_utf8_lossy(std::slice::from_raw_parts(data, (*message).byte_len as usize))
        .into_owned()
}

/// WPOQ3: a definition reports its key to the lock as a key, never a symbol
/// read as a string (the add message named garbage text such as `.rIi`).
#[test]
fn a_symbol_keyed_definition_names_the_symbol_in_the_add_error() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let proto = sealed(BuiltinProto::Object);
        let description = crate::string::js_string_from_bytes(b"desc".as_ptr(), 4);
        let symbol = crate::symbol::js_symbol_new(crate::value::js_nanbox_string(description as i64));
        let bag = crate::object::js_object_alloc(0, 1);
        add_key(bag, b"value", 1.0);
        let thrown = crate::exception::catch_js_throw(|| {
            crate::object::js_object_define_property(
                crate::value::js_nanbox_pointer(proto as i64),
                symbol,
                crate::value::js_nanbox_pointer(bag as i64),
            )
        })
        .expect_err("a program definition of a new symbol key on Object.prototype is refused");
        assert_eq!(
            thrown_message(thrown),
            "Cannot add member Object.prototype[Symbol(desc)]: built-in objects are read-only in a compiled program"
        );
    }
}

/// WPOQ3: a deleted key's tombstone in an overflow table keeps its storage
/// slot but is no member; the store that re-adds the key through the overflow
/// is an add, refused on an add-locked prototype (the lock used to read the
/// tombstone as a key header and crash).
#[test]
fn a_deleted_layout_key_readded_through_the_overflow_is_an_add() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let proto = sealed(BuiltinProto::Object);
        let meta = meta_of(proto);
        let keys = own_keys(proto);
        let pos = image_position_of_key(keys, keys.count(), Key::Bytes(b"k")).expect("premise: k is own");
        let slot = keys.storage_slot(pos) as usize;
        assert_eq!(
            locked_violation(proto, meta, Mutation::Replace(slot)),
            Some(Violation::Replace(".k".into())),
            "premise: k is an image member"
        );
        let key = crate::string::js_string_from_bytes(b"k".as_ptr(), 1);
        crate::object::own_override::as_builtin_definition(|| {
            super::super::overflow::ensure(proto);
            super::super::js_object_delete_field(proto, key);
        });
        assert!(super::super::overflow::has_overflow(proto), "premise: an overflow table");
        assert_eq!(
            locked_violation(proto, meta, Mutation::Replace(slot)),
            None,
            "a tombstone is no member"
        );
        let thrown = crate::exception::catch_js_throw(|| add_key(proto, b"k", 2.0))
            .expect_err("re-adding the deleted key is an add");
        assert_eq!(
            thrown_message(thrown),
            "Cannot add member Object.prototype.k: built-in objects are read-only in a compiled program"
        );
    }
}

/// WPOQ4 cache question: a runtime definition never throws, but one that ADDS
/// a key to an add-locked prototype after its seal changes an absent-key
/// answer already given (the pristine-guarded miss a compiled read takes, the
/// lock's "never gains a key"). It withdraws like the program's write would;
/// the generic read then finds the key. A definition that adds nothing, or an
/// add on a prototype the lock leaves open to adds, is the builtin itself.
#[test]
fn a_definition_add_on_an_add_locked_prototype_withdraws_the_absent_answer() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        for family in [BuiltinProto::Object, BuiltinProto::Function] {
            let proto = sealed(family);
            let child = crate::object::js_object_create(crate::value::js_nanbox_pointer(
                proto as i64,
            ));
            let child = crate::value::js_nanbox_get_pointer(child) as *const ObjectHeader;
            let key = crate::string::js_string_from_bytes(b"q".as_ptr(), 1);
            let read = || crate::object::js_object_get_field_by_name_f64(child, key);
            // Warm: the key is absent and the own bit a guarded miss reads (`pristine` is it plus
            // %Object.prototype%'s, withdrawn by the first family) is clear.
            assert!(
                crate::JSValue::from_bits(read().to_bits()).is_undefined(),
                "{family:?}: premise: q is absent"
            );
            assert!(prototype_unwritten(family), "{family:?}: premise: pristine");
            let meta = meta_of(proto);
            assert!(adds_key(proto, Mutation::Redefine(Key::Bytes(b"absent"))));
            assert!(!adds_key(proto, Mutation::Redefine(Key::Bytes(b"k"))));
            assert!(!adds_key(proto, Mutation::Stamp((*proto).parent_class_id)));
            assert!(!adds_key(proto, Mutation::Replace(0)));
            // A definition that adds nothing keeps the fact.
            crate::object::own_override::as_builtin_definition(|| {
                crate::object::store_object_field_slot(proto, 0, 3.0f64.to_bits());
            });
            assert!(prototype_unwritten(family), "{family:?}: a value refresh is the builtin");
            // The lazy install: a definition-mode add after the seal.
            crate::object::own_override::as_builtin_definition(|| add_key(proto, b"q", 7.0));
            assert!(
                !prototype_unwritten(family),
                "{family:?}: a definition add after the seal withdraws the absent answer"
            );
            assert_eq!(read(), 7.0, "{family:?}: the read sees the installed key");
            assert_eq!(
                locked_violation(proto, meta, Mutation::Add(Key::Bytes(b"p"))),
                Some(Violation::Add(".p".into())),
                "{family:?}: the program stays add-locked"
            );
        }
        // Control: Map's prototype is not add-locked; a definition add there
        // is the builtin's own population and withdraws nothing.
        let map = sealed(BuiltinProto::Map);
        crate::object::own_override::as_builtin_definition(|| add_key(map, b"q", 1.0));
        assert!(prototype_unwritten(BuiltinProto::Map), "a definition add on Map is not a withdrawal");
    }
}

/// WPOQ4: the iterator prototypes carry no fact a definition add could
/// withdraw, so a runtime definition adding a key there after the seal is a
/// runtime bug: `note_sealed_write` aborts with this message (prototype and
/// key named). A definition that adds nothing is the builtin itself.
#[test]
fn a_definition_add_on_a_sealed_iterator_prototype_is_a_runtime_bug() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        for (family, name) in [
            (ITERATOR_FAMILY, "Iterator.prototype"),
            (ARRAY_ITERATOR_FAMILY, "%ArrayIteratorPrototype%"),
        ] {
            let proto = crate::object::js_object_alloc(0, 2);
            add_key(proto, b"k", 1.0);
            seal_object_family(proto, family);
            assert_eq!(
                iterator_definition_add(family, proto, Mutation::Add(Key::Bytes(b"q"))),
                Some(format!(
                    "perry runtime bug: a runtime definition adds {name}.q after the prototype was sealed"
                )),
                "{name}: an add aborts, naming the prototype and the key"
            );
            assert!(
                iterator_definition_add(family, proto, Mutation::Redefine(Key::Bytes(b"absent"))).is_some(),
                "{name}: a definition of a key it does not own is an add"
            );
            assert_eq!(
                iterator_definition_add(family, proto, Mutation::Redefine(Key::Bytes(b"k"))),
                None,
                "{name}: redefining an own key adds nothing"
            );
            assert_eq!(iterator_definition_add(family, proto, Mutation::Replace(0)), None);
            // A value refresh through the definition funnel reaches the gate and survives.
            crate::object::own_override::as_builtin_definition(|| {
                crate::object::store_object_field_slot(proto, 0, 3.0f64.to_bits());
            });
        }
    }
}

/// WPLK1: `%Array.prototype%` keeps no element room (`seal_realm`), so every
/// element a program adds to it needs `js_array_grow`, the one place an
/// existing array's capacity grows, which refuses it: push, unshift and a
/// growing length write called on Array.prototype itself throw and leave it
/// empty, while an ordinary array still grows.
#[test]
fn every_growth_of_array_prototype_is_refused() {
    let _lock = crate::gc::global_side_table_test_lock();
    // The realm (and its sealed %Array.prototype%) exists once the global does.
    crate::object::js_get_global_this();
    let ap = crate::array::array_prototype_addr() as *mut crate::array::ArrayHeader;
    assert!(!ap.is_null());
    unsafe {
        assert_eq!((*ap).capacity, 0, "seal_realm leaves it no element room");
        let apv = crate::value::js_nanbox_pointer(ap as i64);
        let args = [7.0f64, 8.0];
        let refused = |what: &str, f: &dyn Fn()| {
            let thrown = crate::exception::catch_js_throw(f)
                .expect_err(&format!("{what} on Array.prototype is refused"));
            assert!(
                thrown_message(thrown).starts_with("Cannot add an array index to built-in Array.prototype"),
                "{what}: {}",
                thrown_message(thrown)
            );
            assert_eq!((*ap).length, 0, "{what} left Array.prototype empty");
        };
        refused("push", &|| {
            crate::array::js_arraylike_push(apv, args.as_ptr(), 2);
        });
        refused("unshift", &|| {
            crate::array::js_arraylike_unshift(apv, args.as_ptr(), 1);
        });
        refused("push_f64", &|| {
            crate::array::js_array_push_f64(ap, 7.0);
        });
        refused("length = 3", &|| crate::array::js_array_set_length(ap, 3.0));
        let plain = crate::array::js_array_alloc(0);
        let plain = crate::array::js_array_push_f64(plain, 1.0);
        let plain = crate::array::js_array_grow(plain, 64);
        assert!((*plain).capacity >= 64, "an ordinary array still grows");
    }
}

/// WPLK1: `Reflect.set` / `Reflect.defineProperty` report a write the lock
/// refuses as `false` (ECMA-262 28.1.12, 28.1.13); they never throw for it.
/// The decision is the store funnels' own (`lock_refuses_write`).
#[test]
fn reflect_reports_a_refused_write_as_false() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let key = |b: &[u8]| {
            crate::value::js_nanbox_string(crate::string::js_string_from_bytes(b.as_ptr(), b.len() as u32) as i64)
        };
        let proto = sealed(BuiltinProto::Object);
        let pv = crate::value::js_nanbox_pointer(proto as i64);
        assert!(lock_refuses_write(pv, key(b"zz")), "an add on an add-locked prototype");
        assert!(lock_refuses_write(pv, key(b"k")), "an image member");
        assert!(!lock_refuses_write(pv, key(b"constructor")), "constructor is exempt");
        super::super::own_override::as_builtin_definition(|| {
            assert!(!lock_refuses_write(pv, key(b"zz")), "the runtime's own definition");
        });
        crate::object::js_get_global_this();
        let ap = crate::array::array_prototype_addr();
        assert!(ap != 0, "premise: the realm is populated");
        let apv = crate::value::js_nanbox_pointer(ap as i64);
        assert!(lock_refuses_write(apv, 0.0), "an index on Array.prototype");
        assert!(lock_refuses_write(apv, key(b"0")), "an index key string on Array.prototype");
        assert!(!lock_refuses_write(apv, key(b"wplk1Named")), "a named add on Array.prototype");
        let plain = crate::object::js_object_alloc(0, 1);
        assert!(!lock_refuses_write(crate::value::js_nanbox_pointer(plain as i64), key(b"zz")));

        let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
        let set = crate::exception::catch_js_throw(|| {
            crate::proxy::js_reflect_set(pv, key(b"zz"), 1.0, undefined)
        })
        .expect("Reflect.set reports, never throws, a refused write");
        assert_eq!(set.to_bits(), crate::value::TAG_FALSE);
        let bag = crate::object::js_object_alloc(0, 1);
        add_key(bag, b"value", 1.0);
        let defined = crate::exception::catch_js_throw(|| {
            crate::object::property_ops::js_reflect_define_property(
                pv,
                key(b"zz"),
                crate::value::js_nanbox_pointer(bag as i64),
            )
        })
        .expect("Reflect.defineProperty reports, never throws, a refused definition");
        assert_eq!(defined.to_bits(), crate::value::TAG_FALSE);
    }
}

/// WPLK1: `constructor` of a built-in prototype is an exempt key (owner
/// ruling 2026-11-03), not an exempt mutation: after its delete, defining it
/// again is an add of the same key and is admitted
/// (test_gap_11868_object_constructor_intrinsic, test_gap_anon_descriptor_semantics).
#[test]
fn an_exempt_constructor_is_never_refused_as_an_add() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let proto = sealed(BuiltinProto::Object);
        let meta = meta_of(proto);
        assert_eq!(locked_violation(proto, meta, Mutation::Add(Key::Bytes(b"constructor"))), None);
        assert_eq!(
            locked_violation(proto, meta, Mutation::Redefine(Key::Bytes(b"constructor"))),
            None
        );
        assert_eq!(
            locked_violation(proto, meta, Mutation::Add(Key::Bytes(b"other"))),
            Some(Violation::Add(".other".into())),
            "any other add stays refused"
        );
    }
}

/// DEC 250 as narrowed 2026-11-03: `%GeneratorPrototype%` and
/// `%AsyncGeneratorPrototype%` are sealed when their tower is built; their
/// image members are refused, `constructor` and adds are not.
#[test]
fn the_generator_prototypes_lock_their_image_and_take_adds() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    crate::object::ensure_generator_intrinsics();
    for (slot, family, name) in [
        (
            &crate::object::GENERATOR_PROTOTYPE_PTR,
            GENERATOR_FAMILY,
            "%GeneratorPrototype%",
        ),
        (
            &crate::object::ASYNC_GENERATOR_PROTOTYPE_PTR,
            ASYNC_GENERATOR_FAMILY,
            "%AsyncGeneratorPrototype%",
        ),
    ] {
        let proto = slot.load(std::sync::atomic::Ordering::Acquire) as *mut ObjectHeader;
        assert!(!proto.is_null(), "{name}");
        unsafe {
            let meta = meta_of(proto);
            assert_ne!(
                (*meta).flags & super::super::meta_flags::OBJECT_META_FLAG_SEALED_INTRINSIC,
                0,
                "{name}"
            );
            assert_eq!(sealed_family((*meta).flags), family, "{name}");
            for key in [&b"next"[..], b"return", b"throw"] {
                assert!(
                    matches!(
                        locked_violation(proto, meta, Mutation::Delete(Key::Bytes(key))),
                        Some(Violation::Delete(_))
                    ),
                    "{name}: an image member is refused"
                );
            }
            assert_eq!(
                locked_violation(proto, meta, Mutation::Delete(Key::Bytes(b"constructor"))),
                None,
                "{name}: constructor is exempt"
            );
            assert_eq!(
                locked_violation(proto, meta, Mutation::Add(Key::Bytes(b"helper"))),
                None,
                "{name}: an add is allowed"
            );
        }
        assert_eq!(locked_object_name(family), name);
    }
}

/// DEC 250 as narrowed 2026-11-03, like the generator prototypes:
/// `%MapIteratorPrototype%`, `%SetIteratorPrototype%`,
/// `%StringIteratorPrototype%` and `%RegExpStringIteratorPrototype%` are
/// sealed when the iterator tower is built; `next` and `@@toStringTag` are
/// refused, adds are not.
#[test]
fn the_collection_and_string_iterator_prototypes_lock_their_image_and_take_adds() {
    use crate::object::iterator_prototypes as ip;
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    ip::ensure_iterator_prototypes();
    for (slot, family, name) in [
        (
            &ip::MAP_ITERATOR_PROTOTYPE_PTR,
            MAP_ITERATOR_FAMILY,
            "%MapIteratorPrototype%",
        ),
        (
            &ip::SET_ITERATOR_PROTOTYPE_PTR,
            SET_ITERATOR_FAMILY,
            "%SetIteratorPrototype%",
        ),
        (
            &ip::STRING_ITERATOR_PROTOTYPE_PTR,
            STRING_ITERATOR_FAMILY,
            "%StringIteratorPrototype%",
        ),
        (
            &ip::REGEXP_STRING_ITERATOR_PROTOTYPE_PTR,
            REGEXP_STRING_ITERATOR_FAMILY,
            "%RegExpStringIteratorPrototype%",
        ),
    ] {
        let proto = slot.load(std::sync::atomic::Ordering::Acquire) as *mut ObjectHeader;
        assert!(!proto.is_null(), "{name}");
        unsafe {
            let meta = meta_of(proto);
            assert_ne!(
                (*meta).flags & super::super::meta_flags::OBJECT_META_FLAG_SEALED_INTRINSIC,
                0,
                "{name}"
            );
            assert_eq!(sealed_family((*meta).flags), family, "{name}");
            assert!(
                matches!(
                    locked_violation(proto, meta, Mutation::Delete(Key::Bytes(b"next"))),
                    Some(Violation::Delete(_))
                ),
                "{name}: next is refused"
            );
            assert!(
                matches!(
                    locked_violation(proto, meta, Mutation::Redefine(Key::Bytes(b"next"))),
                    Some(Violation::Redefine(_))
                ),
                "{name}: next is refused"
            );
            assert_eq!(
                locked_violation(proto, meta, Mutation::Add(Key::Bytes(b"helper"))),
                None,
                "{name}: an add is allowed"
            );
        }
        assert_eq!(locked_object_name(family), name);
    }
}

/// A method `call_unknown_collection_method` reaches whose body dispatches
/// its own name by name again: a recursion of runtime frames only, no
/// compiled prologue in the cycle.
extern "C" fn redispatching_method_fixture(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    unsafe { call_unknown_collection_method(this.as_f64(), b"foo", &[]) }
}

/// A body that invokes a DIFFERENT name through the same lookup (the
/// `toLocaleString` -> `Invoke(this, "toString")` shape).
extern "C" fn other_name_fixture(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    unsafe { call_unknown_collection_method(this.as_f64(), b"bar", &[]) }
}

extern "C" fn seven_fixture(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    7.0
}

/// The unknown-collection-method lookup is bounded by the agent's stack
/// limit, not by a per-thread flag: a same-name runtime-only cycle throws
/// node's catchable `RangeError: Maximum call stack size exceeded` (a flag
/// guard answers `undefined`, no guard overflows the native stack), and a
/// nested call of another name is an ordinary call.
#[test]
fn unknown_collection_method_recursion_throws_the_stack_range_error() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let scope = crate::gc::RuntimeHandleScope::new();
    crate::stack_guard::publish_stack_limit();
    assert!(
        !crate::agent_ptrs::read(crate::agent_ptrs::AGENT_PTR_STACK_LIMIT).is_null(),
        "premise: this thread has a published stack limit"
    );
    unsafe {
        let obj = crate::object::js_object_alloc(0, 3);
        let recv = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(obj as i64));
        let foo = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::closure::js_closure_alloc(crate::fn_info!(redispatching_method_fixture, 0), 0)
                as i64,
        ));
        let baz = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::closure::js_closure_alloc(crate::fn_info!(other_name_fixture, 0), 0) as i64,
        ));
        let bar = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::closure::js_closure_alloc(crate::fn_info!(seven_fixture, 0), 0) as i64,
        ));
        add_key(obj, b"foo", foo.get_nanbox_f64());
        add_key(obj, b"baz", baz.get_nanbox_f64());
        add_key(obj, b"bar", bar.get_nanbox_f64());

        let nested = call_unknown_collection_method(recv.get_nanbox_f64(), b"baz", &[]);
        assert_eq!(nested, 7.0, "a nested call of another name is an ordinary call");

        let thrown = crate::exception::catch_js_throw(|| {
            call_unknown_collection_method(recv.get_nanbox_f64(), b"foo", &[])
        });
        let exception = match thrown {
            Ok(value) => panic!("a same-name cycle must throw, answered {:#x}", value.to_bits()),
            Err(exception) => exception,
        };
        let message = thrown_message(exception);
        assert!(
            message.contains("Maximum call stack size exceeded"),
            "node's RangeError, got {message:?}"
        );
    }
}
