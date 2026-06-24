use super::*;

#[test]
fn native_library_manifest_lowercase_abi_returns_emit_signatures_and_artifacts() {
    let opts = native_library_opts(vec![
        ("native_ret_jsvalue", vec![], "jsvalue"),
        ("native_ret_string", vec![], "string"),
        ("native_ret_bool", vec![], "bool"),
        ("native_ret_i32", vec![], "i32"),
        ("native_ret_i64", vec![], "i64"),
        ("native_ret_u32", vec![], "u32"),
        ("native_ret_u64", vec![], "u64"),
        ("native_ret_usize", vec![], "usize"),
        ("native_ret_f32", vec![], "f32"),
        ("native_ret_f64", vec![], "f64"),
        ("native_ret_ptr", vec![], "ptr"),
        ("native_ret_buffer_len", vec![], "buffer_len"),
        ("native_ret_handle", vec![], "handle"),
        ("native_ret_promise", vec![], "promise"),
    ]);
    let module = module(
        "artifact_native_library_lowercase_returns.ts",
        vec![
            Stmt::Expr(extern_call("native_ret_jsvalue", Vec::new(), Type::Any)),
            Stmt::Expr(extern_call("native_ret_string", Vec::new(), Type::String)),
            Stmt::Expr(extern_call("native_ret_bool", Vec::new(), Type::Boolean)),
            Stmt::Expr(extern_call("native_ret_i32", Vec::new(), Type::Number)),
            Stmt::Expr(extern_call("native_ret_i64", Vec::new(), Type::Number)),
            Stmt::Expr(extern_call("native_ret_u32", Vec::new(), Type::Number)),
            Stmt::Expr(extern_call("native_ret_u64", Vec::new(), Type::Number)),
            Stmt::Expr(extern_call("native_ret_usize", Vec::new(), Type::Number)),
            Stmt::Expr(extern_call("native_ret_f32", Vec::new(), Type::Number)),
            Stmt::Expr(extern_call("native_ret_f64", Vec::new(), Type::Number)),
            Stmt::Expr(extern_call("native_ret_ptr", Vec::new(), Type::Any)),
            Stmt::Expr(extern_call(
                "native_ret_buffer_len",
                Vec::new(),
                Type::Number,
            )),
            Stmt::Expr(extern_call("native_ret_handle", Vec::new(), Type::Number)),
            Stmt::Return(Some(extern_call(
                "native_ret_promise",
                Vec::new(),
                Type::Number,
            ))),
        ],
    );
    let ir = String::from_utf8(compile_module(&module, opts.clone()).unwrap()).unwrap();
    assert!(
        ir.contains("declare double @native_ret_jsvalue()")
            && ir.contains("declare ptr @native_ret_string()")
            && ir.contains("declare i32 @native_ret_bool()")
            && ir.contains("declare i32 @native_ret_i32()")
            && ir.contains("declare i64 @native_ret_i64()")
            && ir.contains("declare i32 @native_ret_u32()")
            && ir.contains("declare i64 @native_ret_u64()")
            && ir.contains("declare i64 @native_ret_usize()")
            && ir.contains("declare float @native_ret_f32()")
            && ir.contains("declare double @native_ret_f64()")
            && ir.contains("declare ptr @native_ret_ptr()")
            && ir.contains("declare i32 @native_ret_buffer_len()")
            && ir.contains("declare i64 @native_ret_handle()")
            && ir.contains("declare i64 @native_ret_promise()")
            && ir.contains("call double @js_native_handle_new_borrowed"),
        "expected lowercase manifest return kinds to drive LLVM declarations:\n{ir}"
    );

    let artifact = compile_artifact_json_for_module_with_opts(module, opts);
    let records = artifact["records"].as_array().unwrap();
    for (consumer, rep, llvm_ty, abi_kind) in [
        (
            "native_library.raw_jsvalue",
            "js_value",
            "double",
            "jsvalue",
        ),
        (
            "native_library.raw_string",
            "native_handle",
            "i64",
            "string",
        ),
        ("native_library.raw_bool", "i32", "i32", "bool"),
        ("native_library.raw_i32", "i32", "i32", "i32"),
        ("native_library.raw_i64", "i64", "i64", "i64"),
        ("native_library.raw_u32", "u32", "i32", "u32"),
        ("native_library.raw_u64", "u64", "i64", "u64"),
        ("native_library.raw_usize", "usize", "i64", "usize"),
        ("native_library.raw_f32", "f32", "float", "f32"),
        ("native_library.raw_f64", "f64", "double", "f64"),
        ("native_library.raw_ptr", "native_handle", "i64", "ptr"),
        (
            "native_library.raw_buffer_len",
            "buffer_len",
            "i32",
            "buffer_len",
        ),
        (
            "native_library.raw_handle",
            "native_handle",
            "i64",
            "handle",
        ),
        (
            "native_library.raw_promise",
            "promise_boundary",
            "i64",
            "promise",
        ),
    ] {
        assert!(
            records.iter().any(|record| {
                record["expr_kind"] == "NativeLibraryReturn"
                    && record["consumer"] == consumer
                    && record["native_rep_name"] == rep
                    && record["llvm_ty"] == llvm_ty
                    && record["native_value_state"] == "region_local"
                    && record["native_abi_type"]["canonical_kind"] == abi_kind
            }),
            "expected raw native-library return record {consumer}/{rep}:\n{artifact:#}"
        );
    }
    for (consumer, from_rep, op, lossy) in [
        ("materialize_js_value", "u64", "unsigned_int_to_float", true),
        (
            "materialize_js_value",
            "usize",
            "unsigned_int_to_float",
            true,
        ),
        ("materialize_js_value", "f32", "float_extend", false),
        (
            "materialize_native_handle_runtime",
            "native_handle",
            "native_handle_box",
            false,
        ),
        (
            "materialize_promise_boundary",
            "promise_boundary",
            "promise_box",
            false,
        ),
    ] {
        assert!(
            records.iter().any(|record| {
                record["consumer"] == consumer
                    && record["native_value_state"] == "materialized"
                    && record["native_abi_transition"]["from_native_rep"] == from_rep
                    && record["native_abi_transition"]["to_native_rep"] == "js_value"
                    && record["native_abi_transition"]["op"] == op
                    && record["native_abi_transition"]["lossy"] == lossy
            }),
            "expected native-library transition {from_rep}->{op}:\n{artifact:#}"
        );
    }
}

#[test]
fn native_library_manifest_native_async_promise_artifact_records_metadata() {
    let ret = perry_api_manifest::NativeAbiType::Promise(perry_api_manifest::NativePromiseAbi {
        result: Box::new(perry_api_manifest::NativeAbiType::F64),
        completion: perry_api_manifest::NativePromiseCompletion::NativeAsync,
        thread: perry_api_manifest::NativePromiseThread::Main,
    });
    let opts = native_library_opts_typed(vec![("native_ret_native_async", vec![], ret)]);
    let module = module(
        "artifact_native_async_promise_return.ts",
        vec![Stmt::Return(Some(extern_call(
            "native_ret_native_async",
            Vec::new(),
            Type::Number,
        )))],
    );

    let ir = String::from_utf8(compile_module(&module, opts.clone()).unwrap()).unwrap();
    assert!(
        ir.contains("declare i64 @native_ret_native_async()"),
        "native async promise lowering should keep the JS Promise boundary ABI:\n{ir}"
    );

    let artifact = compile_artifact_json_for_module_with_opts(module, opts);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "NativeLibraryReturn"
                && record["consumer"] == "native_library.raw_promise"
                && record["native_rep_name"] == "promise_boundary"
                && record["llvm_ty"] == "i64"
                && record["native_value_state"] == "region_local"
                && record["native_abi_type"]["canonical_kind"] == "promise"
                && record["native_abi_type"]["promise_result"] == "f64"
                && record["native_abi_type"]["promise_completion"] == "native_async"
                && record["native_abi_type"]["promise_thread"] == "main"
        }),
        "expected native async promise ABI metadata in artifact:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["consumer"] == "materialize_promise_boundary"
                && record["native_value_state"] == "materialized"
                && record["native_abi_transition"]["from_native_rep"] == "promise_boundary"
                && record["native_abi_transition"]["to_native_rep"] == "js_value"
                && record["native_abi_transition"]["op"] == "promise_box"
                && record["native_abi_transition"]["lossy"] == false
        }),
        "expected native async promise return to use existing promise boxing:\n{artifact:#}"
    );
}

#[test]
fn native_library_manifest_lowercase_abi_params_emit_c_abi_signature() {
    let opts = native_library_opts(vec![(
        "native_abi_args",
        vec![
            "jsvalue",
            "string",
            "bool",
            "i32",
            "i64",
            "u32",
            "u64",
            "usize",
            "f32",
            "f64",
            "buffer_len",
            "buffer+len",
            "ptr",
            "handle",
            "promise",
        ],
        "void",
    )]);
    let module = module(
        "native_library_lowercase_params.ts",
        vec![
            Stmt::Expr(extern_call(
                "native_abi_args",
                vec![
                    Expr::Number(1.0),
                    Expr::Number(2.0),
                    Expr::Number(3.0),
                    Expr::Number(4.0),
                    Expr::Number(5.0),
                    Expr::Number(6.0),
                    Expr::Number(7.0),
                    Expr::Number(8.0),
                    Expr::Number(9.0),
                    Expr::Number(10.0),
                    Expr::Number(11.0),
                    Expr::Number(12.0),
                    Expr::Number(13.0),
                    Expr::Number(14.0),
                    Expr::Number(15.0),
                ],
                Type::Void,
            )),
            Stmt::Return(Some(int(0))),
        ],
    );
    let ir = String::from_utf8(compile_module(&module, opts.clone()).unwrap()).unwrap();

    assert!(
        ir.contains("call i64 @js_native_abi_check_string_ptr")
            && ir.contains("call i32 @js_native_abi_check_i32")
            && ir.contains("call i64 @js_native_abi_check_i64")
            && ir.contains("call i32 @js_native_abi_check_u32")
            && ir.contains("call i64 @js_native_abi_check_u64")
            && ir.contains("call i64 @js_native_abi_check_usize")
            && ir.contains("call float @js_native_abi_check_f32")
            && ir.contains("call double @js_native_abi_check_f64")
            && ir.contains("call ptr @js_native_abi_check_buffer_data_ptr")
            && ir.contains("call i64 @js_native_abi_check_buffer_byte_len")
            && ir.contains("call i64 @js_native_abi_check_ptr")
            && ir.contains("call i64 @js_native_abi_check_promise")
            && ir.contains("call i64 @js_native_handle_unwrap")
            && ir.contains("call void @native_abi_args(double")
            && ir.contains(
                "declare void @native_abi_args(double, ptr, i32, i32, i64, i32, i64, i64, float, double, i32, ptr, i64, i64, i64, i64)"
            ),
        "expected lowercase manifest param kinds to drive LLVM call/declaration ABI:\n{ir}"
    );

    let artifact = compile_artifact_json_for_module_with_opts(module, opts);
    let records = artifact["records"].as_array().unwrap();
    for (display, abi_slot_index, abi_slot_count) in [
        ("jsvalue", 0, 1),
        ("string", 1, 1),
        ("bool", 2, 1),
        ("i32", 3, 1),
        ("i64", 4, 1),
        ("u32", 5, 1),
        ("u64", 6, 1),
        ("usize", 7, 1),
        ("f32", 8, 1),
        ("f64", 9, 1),
        ("buffer_len", 10, 1),
        ("buffer+len", 11, 2),
        ("buffer+len", 12, 2),
        ("ptr", 13, 1),
        ("handle", 14, 1),
        ("promise<jsvalue>", 15, 1),
    ] {
        assert!(
            records.iter().any(|record| {
                record["expr_kind"] == "NativeLibraryParam"
                    && record["native_abi_type"]["display"] == display
                    && record["native_abi_type"]["direction"] == "param"
                    && record["native_abi_type"]["abi_slot_index"] == abi_slot_index
                    && record["native_abi_type"]["abi_slot_count"] == abi_slot_count
            }),
            "expected native-library param ABI record {display}@{abi_slot_index}:\n{artifact:#}"
        );
    }
    for (display, abi_slot_index, helper) in [
        ("string", 1, "js_native_abi_check_string_ptr"),
        ("bool", 2, "js_is_truthy"),
        ("i32", 3, "js_native_abi_check_i32"),
        ("i64", 4, "js_native_abi_check_i64"),
        ("u32", 5, "js_native_abi_check_u32"),
        ("u64", 6, "js_native_abi_check_u64"),
        ("usize", 7, "js_native_abi_check_usize"),
        ("f32", 8, "js_native_abi_check_f32"),
        ("f64", 9, "js_native_abi_check_f64"),
        ("buffer_len", 10, "js_native_abi_check_u32"),
        ("buffer+len", 11, "js_native_abi_check_buffer_data_ptr"),
        ("buffer+len", 12, "js_native_abi_check_buffer_byte_len"),
        ("ptr", 13, "js_native_abi_check_ptr"),
        ("handle", 14, "js_native_handle_unwrap"),
        ("promise<jsvalue>", 15, "js_native_abi_check_promise"),
    ] {
        assert!(
            records.iter().any(|record| {
                record["expr_kind"] == "NativeLibraryParam"
                    && record["native_abi_type"]["display"] == display
                    && record["native_abi_type"]["abi_slot_index"] == abi_slot_index
                    && record["native_abi_type"]["runtime_guard"]["helper"] == helper
                    && record["materialization_reason"].is_null()
                    && record["native_value_state"] == "region_local"
            }),
            "expected native-library param runtime guard {display}@{abi_slot_index}/{helper}:\n{artifact:#}"
        );
    }
}

#[test]
fn native_library_manifest_json_param_serializes_before_call() {
    // #5626: a `"json"` manifest param JSON-serializes its JS argument at the
    // call site (via `js_json_stringify`) and passes the resulting string
    // pointer through a single `ptr` ABI slot — identical wire shape to a
    // `"string"` param, so the native side `serde_json`-deserializes it
    // unchanged. This is what lets descriptor-object bindings (e.g.
    // `deviceCreateBuffer(d, { size, usage })`) work after #5621 rewrote the
    // call site directly to the FFI symbol, bypassing the TS wrapper body that
    // used to do the `JSON.stringify`.
    let opts = native_library_opts(vec![("native_take_descriptor", vec!["i64", "json"], "i64")]);
    let module = module(
        "native_library_json_param.ts",
        vec![Stmt::Return(Some(extern_call(
            "native_take_descriptor",
            vec![Expr::Number(7.0), Expr::Number(42.0)],
            Type::Number,
        )))],
    );
    let ir = String::from_utf8(compile_module(&module, opts.clone()).unwrap()).unwrap();

    assert!(
        ir.contains("call i64 @js_json_stringify(")
            // The serialized descriptor occupies a `ptr` ABI slot, like `string`.
            && ir.contains("declare i64 @native_take_descriptor(i64, ptr)"),
        "expected json manifest param to stringify and pass a string pointer:\n{ir}"
    );
    // The strict string validator must NOT run for a json param — the whole
    // point is to accept a non-string (object) argument. (It is always
    // `declare`d as a runtime symbol; what must be absent is a *call* to it.)
    assert!(
        !ir.contains("call i64 @js_native_abi_check_string_ptr"),
        "json param must not route through the strict string validator:\n{ir}"
    );

    let artifact = compile_artifact_json_for_module_with_opts(module, opts);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "NativeLibraryParam"
                && record["native_abi_type"]["display"] == "json"
                && record["native_abi_type"]["direction"] == "param"
                && record["native_abi_type"]["abi_slot_index"] == 1
                && record["native_abi_type"]["abi_slot_count"] == 1
                && record["native_abi_type"]["runtime_guard"]["helper"] == "js_json_stringify"
        }),
        "expected native-library json param ABI record:\n{artifact:#}"
    );
}
