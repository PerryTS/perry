//! Depth admission through the tape path and its legacy error fallback.
use super::*;

fn nested_arrays(depth: usize, leaf: &str) -> Vec<u8> {
    let mut input = vec![b'['; depth];
    input.extend_from_slice(leaf.as_bytes());
    input.extend(std::iter::repeat_n(b']', depth));
    input
}

#[test]
fn json_tape_deep_result_is_eager_and_safe_on_a_small_stack() {
    std::thread::Builder::new()
        .name("json-tape-depth".into())
        .stack_size(2 * 1024 * 1024)
        .spawn(|| unsafe {
            const DEPTH: usize = 10_000;
            let input = nested_arrays(DEPTH, "7");
            for typed in [false, true] {
                let text = crate::js_string_from_bytes(input.as_ptr(), input.len() as u32);
                let value = if typed {
                    js_json_parse_typed_array(text, b"x\0".as_ptr(), 2, 1)
                } else {
                    js_json_parse(text)
                };
                let root = crate::gc::RuntimeHandleScope::new();
                let held = root.root_nanbox_u64(value.bits());
                let mut value = JSValue::from_bits(held.get_nanbox_u64());
                for _ in 0..DEPTH {
                    let array = value.as_pointer::<crate::ArrayHeader>();
                    let header = array
                        .cast::<u8>()
                        .sub(crate::gc::GC_HEADER_SIZE)
                        .cast::<crate::gc::GcHeader>();
                    assert_eq!(
                        (*header).obj_type,
                        crate::gc::GC_TYPE_ARRAY,
                        "deep trees must not leave a recursively materialized lazy node"
                    );
                    assert_eq!((*array).length, 1);
                    value = crate::array::js_array_get(array, 0);
                }
                assert_eq!(value.as_number(), 7.0);
            }
        })
        .unwrap()
        .join()
        .expect("deep tape parse must not overflow a worker stack");
}

#[test]
fn json_tape_depth_handoff_keeps_shallow_arrays_lazy() {
    unsafe {
        let limit = crate::json::parser::MAX_RECURSIVE_NESTING_DEPTH;
        for depth in [limit - 1, limit, limit + 1] {
            let input = nested_arrays(depth, "0");
            let text = crate::js_string_from_bytes(input.as_ptr(), input.len() as u32);
            let root = parse_root_push(JSValue::string_ptr(text));
            let value = try_parse_via_tape(root, input.len()).expect("valid boundary tape");
            let header = value
                .as_pointer::<u8>()
                .sub(crate::gc::GC_HEADER_SIZE)
                .cast::<crate::gc::GcHeader>();
            assert_eq!(
                (*header).obj_type,
                if depth > limit {
                    crate::gc::GC_TYPE_ARRAY
                } else {
                    crate::gc::GC_TYPE_LAZY_ARRAY
                },
                "depth {depth}"
            );
            parse_root_restore(root);
        }
    }
}

#[test]
fn json_tape_nonarray_deep_root_uses_iterative_materialization() {
    unsafe {
        let depth = crate::json::parser::MAX_RECURSIVE_NESTING_DEPTH + 1;
        let input = format!(
            "{{\"x\":{}}}",
            String::from_utf8(nested_arrays(depth, "9")).unwrap()
        );
        let text = crate::js_string_from_bytes(input.as_ptr(), input.len() as u32);
        let root = parse_root_push(JSValue::string_ptr(text));
        // Exercise the force-on object route without changing the cached env mode.
        let value = try_parse_via_tape(root, input.len()).expect("valid deep object tape");
        assert!(value.is_pointer());
        let value_root = parse_root_push(value);
        let key = crate::js_string_from_bytes(b"x".as_ptr(), 1);
        let mut child = crate::object::js_object_get_field_by_name(
            parse_root_get(value_root).as_pointer(),
            key,
        );
        for _ in 0..depth {
            child = crate::array::js_array_get(child.as_pointer(), 0);
        }
        assert_eq!(child.as_number(), 9.0);
        parse_root_restore(root);
    }
}

#[test]
fn json_tape_fallback_preserves_syntax_and_budget_errors_across_entries() {
    let depth = crate::json::parser::MAX_RECURSIVE_NESTING_DEPTH + 1;
    let mut trailing = nested_arrays(depth, "0");
    trailing.push(b'x');
    let mut early_invalid = b"[?,".to_vec();
    early_invalid.extend(std::iter::repeat_n(
        b'[',
        crate::json::parser::MAX_ITERATIVE_NESTING_DEPTH + 1,
    ));
    let cases = [
        (
            nested_arrays(depth, "01"),
            crate::error::ERROR_KIND_SYNTAX_ERROR,
        ),
        (trailing, crate::error::ERROR_KIND_SYNTAX_ERROR),
        (
            nested_arrays(crate::json::parser::MAX_ITERATIVE_NESTING_DEPTH + 1, "0"),
            crate::error::ERROR_KIND_RANGE_ERROR,
        ),
        (early_invalid, crate::error::ERROR_KIND_RANGE_ERROR),
    ];
    for (input, kind) in cases {
        for entry in 0..3 {
            let before = parse_root_save_len();
            let text = crate::js_string_from_bytes(input.as_ptr(), input.len() as u32);
            let result = if entry == 0 {
                unsafe { js_json_parse_result(text) }
            } else {
                crate::exception::catch_js_throw(|| unsafe {
                    if entry == 1 {
                        js_json_parse(text)
                    } else {
                        js_json_parse_typed_array(text, b"x\0".as_ptr(), 2, 1)
                    }
                })
            };
            let error = result.expect_err("malformed/depth-limited input must reject");
            let header =
                JSValue::from_bits(error.to_bits()).as_pointer::<crate::error::ErrorHeader>();
            assert_eq!(unsafe { (*header).error_kind }, kind, "entry {entry}");
            assert_eq!(
                parse_root_save_len(),
                before,
                "failed parse must restore its roots"
            );
        }
    }
}
