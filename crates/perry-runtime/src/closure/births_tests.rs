use super::*;
use crate::closure::{js_closure_get_capture_bits, js_closure_get_func, real_capture_count};

extern "C" fn first_body(_closure: *const ClosureHeader, _this: crate::closure::JsThis) -> f64 {
    1.0
}
extern "C" fn second_body(_closure: *const ClosureHeader, _this: crate::closure::JsThis) -> f64 {
    2.0
}

static FIRST: JsFunctionInfo =
    JsFunctionInfo::of(first_body as crate::codegen_abi::JsBody0<ClosureHeader>);
static SECOND: JsFunctionInfo =
    JsFunctionInfo::of(second_body as crate::codegen_abi::JsBody0<ClosureHeader>);

fn record(
    info: &'static JsFunctionInfo,
    captures: u32,
    first: u32,
    dest_slot: u32,
) -> ClosureBirth {
    ClosureBirth {
        info,
        capture_count: captures,
        first_capture: first,
        dest_source: 0,
        dest_slot,
    }
}

/// A table bears one fresh closure per record, in order, with the record's
/// body and the source words its capture indices name, and stores each in
/// its scope slot.
#[test]
fn a_table_bears_each_record_into_its_scope_slot() {
    let scope = crate::r#box::scope::js_scope_alloc(4, crate::value::TAG_UNDEFINED as i64) as u64;
    let scope_word = scope | crate::value::POINTER_TAG;
    let value = crate::value::JSValue::int32(7).bits();
    // Source 0 is the scope object itself, source 1 a plain value.
    let words = [scope_word, value];
    let indices = [0u32, 1, 1];
    let births = [
        record(&FIRST, 2, 0, 3),
        record(&SECOND, 1, 2, 1),
        record(&FIRST, 0, 0, 0),
    ];
    js_closure_births_to_scope(
        births.as_ptr(),
        births.len() as u32,
        indices.as_ptr(),
        indices.len() as u32,
        words.as_ptr(),
        words.len() as u32,
    );
    let slot = |i: usize| unsafe { *(scope as *const u64).add(i) };
    let closure_at = |i: usize| (slot(i) & 0x0000_FFFF_FFFF_FFFF) as *mut ClosureHeader;
    for i in [0, 1, 3] {
        assert_eq!(
            slot(i) & crate::value::TAG_MASK,
            crate::value::POINTER_TAG,
            "slot {i} holds a closure"
        );
    }
    assert_eq!(
        slot(2),
        crate::value::TAG_UNDEFINED,
        "an unnamed slot is untouched"
    );
    let (a, b, c) = (closure_at(3), closure_at(1), closure_at(0));
    assert!(
        a != b && a != c && b != c,
        "every record bears a fresh closure"
    );
    unsafe {
        assert_eq!(real_capture_count((*a).capture_count), 2);
        assert_eq!(real_capture_count((*b).capture_count), 1);
        assert_eq!(real_capture_count((*c).capture_count), 0);
        assert_eq!(
            (*a).shape_id,
            crate::closure::shape::birth_shape_for_body(&FIRST)
        );
    }
    assert_eq!(js_closure_get_capture_bits(a, 0) as u64, scope_word);
    assert_eq!(js_closure_get_capture_bits(a, 1) as u64, value);
    assert_eq!(js_closure_get_capture_bits(b, 0) as u64, value);
    assert_eq!(js_closure_get_func(a), js_closure_get_func(c));
    assert_ne!(js_closure_get_func(a), js_closure_get_func(b));
}

/// A record naming a capture index or source word past its table is refused
/// before anything is stored for it.
#[test]
fn a_malformed_record_stops_the_walk() {
    let scope = crate::r#box::scope::js_scope_alloc(2, crate::value::TAG_UNDEFINED as i64) as u64;
    let words = [scope | crate::value::POINTER_TAG];
    let indices = [5u32];
    let births = [record(&FIRST, 1, 0, 0), record(&FIRST, 0, 0, 1)];
    js_closure_births_to_scope(
        births.as_ptr(),
        births.len() as u32,
        indices.as_ptr(),
        indices.len() as u32,
        words.as_ptr(),
        words.len() as u32,
    );
    for i in 0..2 {
        assert_eq!(
            unsafe { *(scope as *const u64).add(i) },
            crate::value::TAG_UNDEFINED
        );
    }
}
