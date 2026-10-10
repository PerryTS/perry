//! The method site's direct `RegExp.prototype.test` entry is chosen from the
//! receiver's and the holder's shapes alone
//! (`regex_proto_thunks::method_site_test_code`), and every change that can
//! make RegExpExec observe something else changes one of those shapes.
use super::regex_proto_thunks::{method_site_test_code, regex_proto_test_direct};
use super::ObjectHeader;
use crate::value::{js_nanbox_get_pointer, js_nanbox_pointer};

fn key(name: &str) -> *const crate::StringHeader {
    crate::string::js_string_from_str(name)
}

unsafe fn prototype() -> *mut ObjectHeader {
    js_nanbox_get_pointer(crate::regex::instance::intrinsic_prototype()) as *mut ObjectHeader
}

unsafe fn method(holder: *mut ObjectHeader, name: &str) -> f64 {
    super::js_object_get_field_by_name_f64(holder, key(name))
}

unsafe fn info_of(value: f64) -> &'static crate::closure::JsFunctionInfo {
    let closure = js_nanbox_get_pointer(value) as *const crate::closure::ClosureHeader;
    &*(*closure).info
}

/// The code a site priming `re.test(s)` (one argument) would publish.
unsafe fn code(re: *mut ObjectHeader) -> u64 {
    let proto = prototype();
    let holder = super::shapes::object_shape_descriptor(proto).expect("prototype shape");
    method_site_test_code(info_of(method(proto, "test")), word(re), &holder, 1)
}

/// The receiver word a site compares (class id | ShapeId << 32).
unsafe fn word(re: *mut ObjectHeader) -> u64 {
    (re as *const u64).read()
}

fn direct() -> u64 {
    regex_proto_test_direct as *const () as u64
}

fn in_fresh_realm(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(move || {
            let _stable = crate::gc::GcSuppressScope::new();
            f()
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn regexp_prototype_shape_names_the_builtin_exec_and_test_bodies() {
    in_fresh_realm(|| unsafe {
        let re = crate::regex::test_construct_regexp_and_exec_once("a", "");
        let proto = prototype();
        let shape = super::shapes::object_shape_descriptor(proto).expect("prototype shape");
        for name in ["exec", "test"] {
            let keys = shape.keys as usize as *const crate::array::ArrayHeader;
            let slot = super::keys_find_slot_by_bytes_resolved(
                keys,
                shape.logical_key_count,
                name.as_bytes(),
            )
            .expect("an own method");
            assert_ne!(
                shape.special_constfn_mask & (1 << slot),
                0,
                "{name} is a ConstFn lane"
            );
        }
        assert_eq!(
            code(re),
            direct(),
            "a literal-born RegExp takes the direct entry"
        );
        let argc2 = method_site_test_code(info_of(method(proto, "test")), word(re), &shape, 2);
        assert_ne!(argc2, direct(), "only the one-argument call is direct");
    });
}

#[test]
fn an_own_exec_takes_the_receiver_off_the_direct_entry() {
    in_fresh_realm(|| unsafe {
        let re = crate::regex::test_construct_regexp_and_exec_once("a", "");
        assert_eq!(code(re), direct());
        let other = method(prototype(), "toString");
        super::js_object_set_field_by_name(re, key("exec"), other);
        assert_ne!(code(re), direct(), "own exec: RegExpExec must call it");
        let fresh = crate::regex::test_construct_regexp_and_exec_once("a", "");
        assert_eq!(code(fresh), direct(), "only that receiver's shape changed");
    });
}

#[test]
fn a_replaced_prototype_exec_takes_every_receiver_off_the_direct_entry() {
    in_fresh_realm(|| unsafe {
        let re = crate::regex::test_construct_regexp_and_exec_once("a", "");
        assert_eq!(code(re), direct());
        let proto = prototype();
        let before = super::shapes::object_shape_stamp(proto);
        let builtin = method(proto, "exec");
        let other = method(proto, "toString");
        super::js_object_set_field_by_name(proto, key("exec"), other);
        assert_ne!(
            super::shapes::object_shape_stamp(proto),
            before,
            "the store revoked the lane"
        );
        assert_ne!(code(re), direct(), "a replaced exec is observable");
        // Restoring the builtin does not re-learn the lane: the thunk's own
        // proof answers from then on.
        super::js_object_set_field_by_name(proto, key("exec"), builtin);
        assert_ne!(code(re), direct());
        let fresh = crate::regex::test_construct_regexp_and_exec_once("a", "g");
        assert_ne!(code(fresh), direct());
    });
}

#[test]
fn a_non_regexp_receiver_never_takes_the_direct_entry() {
    in_fresh_realm(|| unsafe {
        let _ = crate::regex::test_construct_regexp_and_exec_once("a", "");
        let plain = super::js_object_alloc(0, 2);
        super::prototype_chain::object_set_static_prototype(
            plain as usize,
            js_nanbox_pointer(prototype() as i64).to_bits(),
        );
        assert_ne!(
            code(plain),
            direct(),
            "no matcher: the builtin exec must throw"
        );
    });
}

/// The direct entry is the builtin `test` itself, not a second search: for
/// every flag combination, `lastIndex` value and heap subject (one past
/// 64 KiB), it answers and leaves `lastIndex` exactly as the generic thunk
/// does on an identical receiver.
#[test]
fn the_direct_entry_answers_as_the_generic_test() {
    in_fresh_realm(|| {
        let long = format!(
            "{}needle{}needle",
            "a".repeat(64 * 1024 + 7),
            "b".repeat(100)
        );
        let subjects = [
            "a heap subject with a needle in it".to_string(),
            long,
            "a heap subject with no match at all".to_string(),
        ];
        for flags in ["", "g", "y", "gy"] {
            for subject in &subjects {
                for start in [0.0, 3.0, 65_543.0, 65_549.0, 1e12, -1.0] {
                    let mut seen = Vec::new();
                    for direct_entry in [false, true] {
                        let re = crate::regex::test_construct_regexp_and_exec_once("needle", flags);
                        crate::regex::set_last_index(re, start);
                        let s = crate::value::js_nanbox_string(crate::string::js_string_from_str(
                            subject,
                        ) as i64);
                        let this = crate::closure::JsThis::from_f64(js_nanbox_pointer(re as i64));
                        let matched = if direct_entry {
                            regex_proto_test_direct(std::ptr::null(), this, s)
                        } else {
                            super::regex_proto_thunks::regex_proto_test_thunk(
                                std::ptr::null(),
                                this,
                                s,
                            )
                        };
                        seen.push((
                            matched.to_bits(),
                            crate::regex::get_last_index(re).to_bits(),
                        ));
                    }
                    assert_eq!(
                        seen[0],
                        seen[1],
                        "/needle/{flags} lastIndex {start} over {} units: direct vs generic",
                        subject.len()
                    );
                }
            }
        }
    });
}
