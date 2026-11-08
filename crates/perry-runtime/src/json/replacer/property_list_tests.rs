use super::*;

fn key(name: &str) -> *mut StringHeader {
    crate::string::canonical_key(name.as_bytes())
}

unsafe fn parse(source: &str) -> JSValue {
    crate::json::test_json_parse_direct(js_string_from_bytes(source.as_ptr(), source.len() as u32))
}

fn text(bits: i64) -> String {
    let value = JSValue::from_bits(bits as u64);
    if value.is_short_string() {
        let mut bytes = [0; crate::value::SHORT_STRING_MAX_LEN];
        let n = value.short_string_to_buf(&mut bytes);
        return String::from_utf8(bytes[..n].to_vec()).unwrap();
    }
    unsafe { str_from_header((bits as u64 & POINTER_MASK) as *const StringHeader) }
        .unwrap()
        .to_owned()
}

extern "C" fn throwing_getter(
    _closure: *const crate::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    crate::exception::throw_core(crate::error::CoreError::type_error(key("excluded getter")))
}

unsafe fn define_getter(object: JSValue, name: &str, info: *const crate::closure::JsFunctionInfo) {
    let getter = crate::closure::js_closure_alloc(info, 0);
    crate::object::js_object_define_getter(
        f64::from_bits(object.bits()),
        crate::value::nanbox_string_key(key(name)),
        crate::value::js_nanbox_pointer(getter as i64),
    );
}

#[test]
fn property_list_skips_excluded_throwing_getter() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    unsafe {
        let object = parse(r#"{"keep":1,"excluded":2}"#);
        define_getter(object, "excluded", crate::fn_info!(throwing_getter, 0));
        let whitelist = parse(r#"["keep"]"#);
        let result = crate::exception::catch_js_throw(|| {
            crate::json::js_json_stringify_full(
                f64::from_bits(object.bits()),
                f64::from_bits(whitelist.bits()),
                f64::from_bits(TAG_UNDEFINED),
            )
        });
        assert!(result.is_ok(), "an excluded getter must not execute");
        assert_eq!(text(result.unwrap()), r#"{"keep":1}"#);
    }
}

unsafe fn stringify(object: JSValue, whitelist: &str, spacer: f64) -> String {
    let whitelist = parse(whitelist);
    text(crate::json::js_json_stringify_full(
        f64::from_bits(object.bits()),
        f64::from_bits(whitelist.bits()),
        spacer,
    ))
}

#[test]
fn property_list_empty_whitelist_never_reads_getters() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    unsafe {
        let object = parse(r#"{"excluded":2}"#);
        define_getter(object, "excluded", crate::fn_info!(throwing_getter, 0));
        let result = crate::exception::catch_js_throw(|| {
            stringify(object, "[]", f64::from_bits(TAG_UNDEFINED))
        });
        assert_eq!(result.unwrap(), "{}");
    }
}

thread_local! {
    static READS: std::cell::RefCell<Vec<&'static str>> = const {
        std::cell::RefCell::new(Vec::new())
    };
}

extern "C" fn getter_a(
    _closure: *const crate::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    READS.with(|reads| reads.borrow_mut().push("a"));
    1.0
}

extern "C" fn getter_b(
    _closure: *const crate::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    READS.with(|reads| reads.borrow_mut().push("b"));
    2.0
}

#[test]
fn property_list_reads_getters_in_whitelist_order() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    unsafe {
        let object = parse(r#"{"b":0,"a":0}"#);
        define_getter(object, "b", crate::fn_info!(getter_b, 0));
        define_getter(object, "a", crate::fn_info!(getter_a, 0));
        for spacer in [f64::from_bits(TAG_UNDEFINED), 2.0] {
            READS.with(|reads| reads.borrow_mut().clear());
            let output = stringify(object, r#"["a","b","a"]"#, spacer);
            let expected = if spacer == 2.0 {
                "{\n  \"a\": 1,\n  \"b\": 2\n}"
            } else {
                r#"{"a":1,"b":2}"#
            };
            assert_eq!(output, expected);
            READS.with(|reads| assert_eq!(*reads.borrow(), ["a", "b"]));
        }
    }
}

extern "C" fn mutating_getter(
    _closure: *const crate::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    let object = crate::value::js_nanbox_get_pointer(this.as_f64()) as *mut crate::ObjectHeader;
    crate::object::js_object_set_field_by_name(object, key("state"), 7.0);
    crate::object::js_object_set_field_by_name(object, key("added"), 9.0);
    1.0
}

extern "C" fn reading_mutation_getter(
    _closure: *const crate::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    let object = crate::value::js_nanbox_get_pointer(this.as_f64()) as *const crate::ObjectHeader;
    f64::from_bits(crate::object::js_object_get_field_by_name(object, key("state")).bits())
}

#[test]
fn property_list_observes_mutations_before_later_reads() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    unsafe {
        let object = parse(r#"{"b":0,"a":0,"state":0}"#);
        define_getter(object, "b", crate::fn_info!(reading_mutation_getter, 0));
        define_getter(object, "a", crate::fn_info!(mutating_getter, 0));
        assert_eq!(
            stringify(
                object,
                r#"["a","b","added"]"#,
                f64::from_bits(TAG_UNDEFINED)
            ),
            r#"{"a":1,"b":7,"added":9}"#,
        );
        let receiver = object.as_pointer::<crate::ObjectHeader>();
        assert_eq!(
            crate::object::js_object_get_field_by_name(receiver, key("state")).as_number(),
            7.0
        );
        assert_eq!(
            crate::object::js_object_get_field_by_name(receiver, key("added")).as_number(),
            9.0
        );
    }
}

#[test]
fn property_list_included_throw_stops_later_getters() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    unsafe {
        let object = parse(r#"{"excluded":0,"b":0}"#);
        define_getter(object, "excluded", crate::fn_info!(throwing_getter, 0));
        define_getter(object, "b", crate::fn_info!(getter_b, 0));
        READS.with(|reads| reads.borrow_mut().clear());
        let result = crate::exception::catch_js_throw(|| {
            stringify(object, r#"["excluded","b"]"#, f64::from_bits(TAG_UNDEFINED))
        });
        assert!(
            result.is_err(),
            "an included throwing getter must propagate"
        );
        READS.with(|reads| assert!(reads.borrow().is_empty(), "throw stops later reads"));
    }
}

#[test]
fn property_list_keeps_nested_arrays_pretty_output_and_overflow_fields() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    unsafe {
        let object = parse(
            r#"{"a":[{"a":1,"omit":2}],"omit":3,"c":3,"d":4,"e":5,"f":6,"g":7,"h":8,"i":9,"j":10}"#,
        );
        assert_eq!(
            stringify(
                object,
                r#"["j","a","missing","j"]"#,
                f64::from_bits(TAG_UNDEFINED)
            ),
            r#"{"j":10,"a":[{"a":1}]}"#,
        );
        assert_eq!(
            stringify(object, r#"["j","a","missing","j"]"#, 2.0),
            "{\n  \"j\": 10,\n  \"a\": [\n    {\n      \"a\": 1\n    }\n  ]\n}",
        );
    }
}

thread_local! {
    static NESTED_PARENT: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}
extern "C" fn mutate_parent_later_getter(
    _closure: *const crate::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    NESTED_PARENT.with(|p| {
        crate::object::js_object_set_field_by_name(
            JSValue::from_bits(p.get())
                .as_pointer::<crate::ObjectHeader>()
                .cast_mut(),
            key("later"),
            7.0,
        )
    });
    1.0
}

#[test]
fn property_list_nested_serialization_mutates_later_parent_read() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    unsafe {
        let parent = parse(r#"{"child":{"a":0},"later":0}"#);
        let child = crate::object::js_object_get_field_by_name(
            parent.as_pointer::<crate::ObjectHeader>(),
            key("child"),
        );
        NESTED_PARENT.with(|p| p.set(parent.bits()));
        define_getter(child, "a", crate::fn_info!(mutate_parent_later_getter, 0));
        let output = stringify(
            parent,
            r#"["child","a","later"]"#,
            f64::from_bits(TAG_UNDEFINED),
        );
        assert_eq!(output, r#"{"child":{"a":1},"later":7}"#);
    }
}

#[test]
fn property_list_preserves_public_private_storage_hiding() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    unsafe {
        for class_id in [0, 11232] {
            let object =
                parse(r#"{"visible":1,"__perry_cap_45m0000331fa678":2,"__perry_cap_7":3}"#);
            (*object.as_pointer::<crate::ObjectHeader>().cast_mut()).class_id = class_id;
            let output = stringify(
                object,
                r#"["visible","__perry_cap_45m0000331fa678","__perry_cap_7"]"#,
                f64::from_bits(TAG_UNDEFINED),
            );
            assert_eq!(
                output,
                if class_id == 0 {
                    r#"{"visible":1,"__perry_cap_45m0000331fa678":2,"__perry_cap_7":3}"#
                } else {
                    r#"{"visible":1,"__perry_cap_7":3}"#
                }
            );
        }
    }
}
