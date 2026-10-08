//! The runtime's stream state record (`node_stream/state_record.rs`) is
//! reached only through its stream: a plain stream's `meta.native_state`, or
//! a stream family's payload cell. A moving collection must keep every value
//! the record names alive and rewrite each edge to the copy. Sabotages (run
//! from `every_stream_sabotage_makes_its_witness_red`): `record_trace` (the
//! meta record's and the cell's GC arms skip the record edge) and
//! `record_barrier` (a record store skips its write barrier).
use crate::node_stream::native_hooks::tests::{new_rot13, FamilyReset, Rot13Opts};
use crate::node_stream::{test_record_slot_bits, test_write_inert_slot, test_read_inert_slot};
use crate::value::{JSValue, TAG_TRUE, TAG_UNDEFINED};

fn key(name: &str) -> *mut crate::string::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn handle(stream: f64) -> i64 {
    (stream.to_bits() & crate::value::POINTER_MASK) as i64
}

/// A young object `{ tag: n, text: "chunk-…" }`.
fn chunk(n: u32) -> f64 {
    let obj = crate::object::js_object_alloc(0, 2);
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(obj);
    crate::object::js_object_set_field_by_name(obj.get_raw_mut_ptr(), key("tag"), f64::from(n));
    let text = format!("chunk-{n:06}-payload-payload");
    let text = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
    crate::object::js_object_set_field_by_name(
        obj.get_raw_mut_ptr(),
        key("text"),
        f64::from_bits(JSValue::string_ptr(text).bits()),
    );
    crate::value::js_nanbox_pointer(obj.get_raw_mut_ptr::<crate::object::ObjectHeader>() as i64)
}

fn field(obj: f64, name: &str) -> f64 {
    let ptr = (obj.to_bits() & crate::value::POINTER_MASK) as *const crate::object::ObjectHeader;
    crate::object::js_object_get_field_by_name_f64(ptr, key(name))
}

fn assert_chunk(got: f64, n: u32, what: &str) {
    assert_eq!(field(got, "tag"), f64::from(n), "{what}: chunk {n} intact after the move");
    let mut bytes = Vec::new();
    crate::node_stream::test_append_chunk_bytes(field(got, "text"), &mut bytes);
    assert_eq!(
        bytes,
        format!("chunk-{n:06}-payload-payload").into_bytes(),
        "{what}: chunk {n}'s string intact"
    );
}

fn object_mode_readable() -> f64 {
    let opts = crate::object::js_object_alloc(0, 1);
    crate::object::js_object_set_field_by_name(opts, key("objectMode"), f64::from_bits(TAG_TRUE));
    crate::node_stream::js_node_stream_readable_new(crate::value::js_nanbox_pointer(opts as i64))
}

/// A copying minor plus young garbage that reuses what it left behind.
fn collect_and_churn() {
    let copies = crate::gc::copying_minor_cycles();
    crate::gc::gc_collect_minor();
    for i in 0..4000 {
        let s = format!("garbage-garbage-garbage-{i:08}");
        let _ = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
    }
    assert!(
        crate::gc::copying_minor_cycles() > copies,
        "test premise: a copying minor ran"
    );
}

/// Young streams (record moves with them), then tenured streams whose old
/// record receives young values (only the barrier tells the minor), for a
/// plain object-mode Readable and a rot13 family stream.
#[test]
fn stream_record_survives_a_moving_collection() {
    struct Barriers;
    impl Drop for Barriers {
        fn drop(&mut self) {
            crate::gc::js_gc_write_barriers_emitted(0);
        }
    }
    let _reset = FamilyReset::new();
    crate::gc::js_gc_write_barriers_emitted(1);
    let _barriers = Barriers;
    let _no_stack = super::support::ConservativeScanDisabledGuard::new();
    let _poison =
        crate::arena::ProtectionModeGuard::set(crate::arena::FromSpaceProtection::PoisonOnly);
    let scope = crate::gc::RuntimeHandleScope::new();
    const N: u32 = 16;

    // Phase 1: everything young.
    let plain = scope.root_nanbox_f64(object_mode_readable());
    let rot = scope.root_nanbox_f64(new_rot13(Rot13Opts::default()));
    for (i, s) in [&plain, &rot].into_iter().enumerate() {
        assert!(
            test_record_slot_bits(s.get_nanbox_f64()).is_some(),
            "a constructed stream carries its state record"
        );
        test_write_inert_slot(s.get_nanbox_f64(), chunk(1000 + i as u32));
    }
    for n in 0..N {
        crate::node_stream::js_node_stream_method_push(handle(plain.get_nanbox_f64()), chunk(n));
    }
    let before = test_record_slot_bits(plain.get_nanbox_f64()).unwrap();
    collect_and_churn();
    assert_ne!(
        before,
        test_record_slot_bits(plain.get_nanbox_f64()).unwrap(),
        "test premise: the record's values moved and its slots were rewritten"
    );
    for (i, s) in [&plain, &rot].into_iter().enumerate() {
        assert_chunk(test_read_inert_slot(s.get_nanbox_f64()), 1000 + i as u32, "young record");
    }
    for n in 0..N {
        let got = crate::node_stream::js_node_stream_method_read(
            handle(plain.get_nanbox_f64()),
            f64::from_bits(TAG_UNDEFINED),
        );
        assert_chunk(got, n, "young readable");
    }

    // Phase 2: the streams and their records tenure, then take young values.
    for _ in 0..6 {
        crate::gc::gc_collect_minor();
    }
    for (i, s) in [&plain, &rot].into_iter().enumerate() {
        test_write_inert_slot(s.get_nanbox_f64(), chunk(2000 + i as u32));
    }
    for n in 0..N {
        crate::node_stream::js_node_stream_method_push(handle(plain.get_nanbox_f64()), chunk(n));
    }
    collect_and_churn();
    collect_and_churn();
    for (i, s) in [&plain, &rot].into_iter().enumerate() {
        assert_chunk(test_read_inert_slot(s.get_nanbox_f64()), 2000 + i as u32, "old record");
    }
    for n in 0..N {
        let got = crate::node_stream::js_node_stream_method_read(
            handle(plain.get_nanbox_f64()),
            f64::from_bits(TAG_UNDEFINED),
        );
        assert_chunk(got, n, "old readable");
    }
}

/// One `native_state` word, one owner: an object that is both a runtime
/// stream and a native-this alias (`http.ServerResponse.call(this, req)`,
/// `http.Server.call(this, …)` on an object the runtime keeps stream state
/// for) holds the alias in its state record. Whichever comes first, both
/// stay readable, and both survive a moving collection. Sabotage
/// `record_displaces_alias`: a record refuses an aliased object (its state
/// is dropped).
#[test]
fn a_stream_record_and_a_native_this_alias_share_one_word() {
    use crate::object::native_this_alias::{alias_handle_for_object, register_this_to_handle_alias};
    const HANDLE: i64 = 0xe1727;
    let _no_stack = super::support::ConservativeScanDisabledGuard::new();
    let scope = crate::gc::RuntimeHandleScope::new();
    let handle = crate::value::js_nanbox_pointer(HANDLE);
    let undefined = f64::from_bits(TAG_UNDEFINED);
    let alias_of = |v: f64| alias_handle_for_object(v).map(|(h, c)| (h.to_bits(), c));
    let plain = || {
        scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::js_object_alloc(0, 0) as i64,
        ))
    };

    // Alias first, then a stream constructor body (`Writable.call(this)`).
    let a = plain();
    register_this_to_handle_alias(a.get_nanbox_f64(), handle, true);
    crate::node_stream::js_node_stream_writable_subclass_init(a.get_nanbox_f64(), undefined);
    // Stream first, then the alias.
    let b = plain();
    crate::node_stream::js_node_stream_readable_subclass_init(b.get_nanbox_f64(), undefined);
    register_this_to_handle_alias(b.get_nanbox_f64(), handle, false);
    // Alias first, then one runtime state store (a pipe/finished write on a
    // light-my-request Response).
    let c = plain();
    register_this_to_handle_alias(c.get_nanbox_f64(), handle, true);
    test_write_inert_slot(c.get_nanbox_f64(), chunk(7));

    let check = |what: &str| {
        for (s, composite) in [(&a, true), (&b, false), (&c, true)] {
            assert_eq!(
                alias_of(s.get_nanbox_f64()),
                Some((handle.to_bits(), composite)),
                "{what}: the alias survives"
            );
            assert!(
                test_record_slot_bits(s.get_nanbox_f64()).is_some(),
                "{what}: the stream state survives"
            );
        }
        assert!(crate::node_stream::is_classic_stream_instance_value(a.get_nanbox_f64()));
        assert!(crate::node_stream::is_classic_stream_instance_value(b.get_nanbox_f64()));
        assert_chunk(test_read_inert_slot(c.get_nanbox_f64()), 7, what);
    };
    check("fresh");
    crate::gc::gc_collect_minor();
    crate::gc::js_gc_collect();
    check("after collections");
}
