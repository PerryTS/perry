//! K3 C3: the receiver-family check (`family_this_ok`), on runtime objects.
use super::*;
use crate::object::{self, prototype_chain as chain};

/// A body info with the home-key record `m` (a string key) after it.
#[repr(C)]
struct WithRecord {
    info: JsFunctionInfo,
    record: [u8; 6],
}

unsafe extern "C" fn body(_c: *const ClosureHeader, _t: crate::closure::JsThis, _a: f64) -> f64 {
    0.0
}

fn family_fn() -> (&'static JsFunctionInfo, f64) {
    let mut info = JsFunctionInfo::of(body as perry_abi::JsBody1<ClosureHeader>);
    info.family = std::mem::offset_of!(WithRecord, record) as i32;
    let rec: &'static WithRecord = Box::leak(Box::new(WithRecord {
        info,
        // one key: tag 0 (string), length 1, "m"
        record: [1, 0, FAMILY_KEY_STRING, 1, 0, b'm'],
    }));
    let c = crate::closure::js_closure_alloc(&rec.info, 0);
    (&rec.info, crate::value::js_nanbox_pointer(c as i64))
}

fn obj() -> *mut object::ObjectHeader {
    object::js_object_alloc(0, 0)
}

fn put(o: *mut object::ObjectHeader, k: &str, v: f64) {
    let key = crate::string::js_string_from_bytes(k.as_ptr(), k.len() as u32);
    object::js_object_set_field_by_name(o, key, v);
}

fn val(o: *mut object::ObjectHeader) -> f64 {
    crate::value::js_nanbox_pointer(o as i64)
}

fn set_proto(o: *mut object::ObjectHeader, p: *mut object::ObjectHeader) {
    chain::object_set_user_prototype(o as usize, val(p).to_bits());
}

#[test]
fn family_this_on_its_chain_passes() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let (info, f) = family_fn();
    let proto = obj();
    put(proto, "m", f);
    let inst = obj();
    set_proto(inst, proto);
    assert!(
        family_this_ok(info, val(inst)),
        "an object whose prototype holds m"
    );
    assert!(
        family_this_ok(info, val(proto)),
        "a plain prototype holding m (a copy target)"
    );
    // Object.create(instance): its chain holds m.
    let o = obj();
    set_proto(o, inst);
    assert!(family_this_ok(info, val(o)), "Object.create(instance)");
}

#[test]
fn plain_holder_under_another_key_raises() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let (info, f) = family_fn();
    let h = obj();
    put(h, "x", f);
    assert!(
        !family_this_ok(info, val(h)),
        "a plain holder under a non-home key"
    );
    assert!(!family_this_ok(info, val(obj())), "an unrelated object");
    // Copied onto a prototype under another key: its heirs are members.
    let c = obj();
    set_proto(c, h);
    assert!(
        family_this_ok(info, val(c)),
        "an object above this holds f under any key"
    );
    assert!(!family_this_ok(info, 1.5), "a primitive");
}

#[test]
fn nullish_passes() {
    let (info, _) = family_fn();
    assert!(family_this_ok(
        info,
        f64::from_bits(crate::value::TAG_UNDEFINED)
    ));
    assert!(family_this_ok(info, f64::from_bits(crate::value::TAG_NULL)));
}

#[test]
fn declared_class_prototype_raises() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    const CID: u32 = 0x6B45_5A21;
    let (info, f) = family_fn();
    let proto = object::js_object_alloc(CID, 0);
    object::class_decl_prototype_object_root_store(CID, proto);
    put(proto, "m", f);
    assert!(
        !family_this_ok(info, val(proto)),
        "the bare class prototype"
    );
    let inst = obj();
    set_proto(inst, proto);
    assert!(family_this_ok(info, val(inst)), "an instance of the class");
}

#[test]
fn a_body_without_family_is_never_checked() {
    let info = JsFunctionInfo::of(body as perry_abi::JsBody1<ClosureHeader>);
    assert_eq!(info.family, 0);
}

/// CWN ruling K3BUILD13 2: an iteration site checks its fixed `thisArg` once
/// per loop, at the first call; a plain-call site and a family-free body
/// never check.
#[test]
fn handed_loop_site_checks_once_at_its_first_call() {
    use super::super::direct::HandedThis;
    let _no_move = crate::gc::GcSuppressScope::new();
    let (_, f) = family_fn();
    let c = crate::value::js_nanbox_get_pointer(f) as *const ClosureHeader;
    assert_eq!(
        crate::closure::DirectCall1::resolve(c).handed_state(),
        HandedThis::PlainOnly
    );
    let site = crate::closure::DirectCall1::resolve_handed(c);
    assert_eq!(
        site.handed_state(),
        HandedThis::Pending,
        "no check before the first call"
    );
    let proto = obj();
    put(proto, "m", f);
    let inst = obj();
    set_proto(inst, proto);
    site.call(c, crate::closure::JsThis::from_f64(val(inst)), 0.0);
    assert_eq!(
        site.handed_state(),
        HandedThis::Free,
        "checked once, then free"
    );
    let plain_info: &'static JsFunctionInfo = Box::leak(Box::new(JsFunctionInfo::of(
        body as perry_abi::JsBody1<ClosureHeader>,
    )));
    let plain = crate::closure::js_closure_alloc(plain_info, 0);
    assert_eq!(
        crate::closure::DirectCall1::resolve_handed(plain).handed_state(),
        HandedThis::Free
    );
}
