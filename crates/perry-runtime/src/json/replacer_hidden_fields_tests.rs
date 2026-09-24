use super::*;

extern "C" fn identity(_closure: *const crate::ClosureHeader, _key: f64, value: f64) -> f64 {
    value
}

unsafe fn check_hidden_fields(mode: u8) {
    let input =
        br#"{"visible":1,"__perry_cap_45m0000331fa678":2,"__perry_cap_7":3,"__perry_cap_user":4}"#;
    let source = js_string_from_bytes(input.as_ptr(), input.len() as u32);
    let value = crate::json::test_json_parse_direct(source);
    let scope = crate::gc::RuntimeHandleScope::new();
    let object = scope.root_raw_mut_ptr(value.as_pointer::<crate::ObjectHeader>().cast_mut());
    let replacer =
        scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(identity as *const u8, 0));
    let keys: Vec<String> = [
        "visible",
        "__perry_cap_45m0000331fa678",
        "__perry_cap_7",
        "__perry_cap_user",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    // Identical physical slots: runtime-only names are hidden on class instances,
    // while ordinary objects keep even an exact reserved-looking spelling.
    for class_id in [0, 11232] {
        object.with_mut_ptr(|obj: *mut crate::ObjectHeader| (*obj).class_id = class_id);
        let mut output = String::new();
        object.with_const_ptr(|ptr: *const u8| match mode {
            0 => stringify_object_pretty(ptr, &mut output, " ", 0),
            1 => replacer.with_const_ptr(|replacer| {
                stringify_object_with_replacer_pretty(ptr, replacer, &mut output, "", 0)
            }),
            _ => stringify_object_with_array_replacer(ptr, &keys, &mut output, "", 0, false),
        });
        let expected = if class_id == 0 {
            std::str::from_utf8(input).unwrap()
        } else {
            r#"{"visible":1,"__perry_cap_user":4}"#
        };
        assert_eq!(
            output.replace(['\n', ' '], ""),
            expected,
            "mode {mode}, class {class_id}"
        );
    }
}

#[test]
fn pretty_hides_class_capture_fields() {
    unsafe { check_hidden_fields(0) }
}

#[test]
fn function_replacer_hides_class_capture_fields() {
    unsafe { check_hidden_fields(1) }
}

#[test]
fn array_replacer_hides_class_capture_fields() {
    unsafe { check_hidden_fields(2) }
}
