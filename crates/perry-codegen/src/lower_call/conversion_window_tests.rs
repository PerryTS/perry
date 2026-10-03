//! IR-level regression coverage for #11830: the CONVERSION window of a call.
//!
//! #11789 / #11829 rooted every call argument across the evaluation of the
//! arguments after it. What it left was the second window inside a call that
//! takes RAW string pointers: turning an argument into its ABI pointer is itself
//! a collecting step (a value that is not already a heap string is
//! materialised, formatted, or `JSON.stringify`d through a user `toJSON`), so
//! converting the arguments in order stranded each earlier raw pointer one
//! collection from stale.
//!
//! Every test makes the FIRST operand a freshly built object (a non-literal
//! operand, so it needs a conversion) and the SECOND another one, then asks
//! [`assert_rooted_across`] to prove the first operand's CONVERTED pointer was
//! parked in a rooted slot before the second conversion ran, and that the
//! consuming call read it back out of that slot. The runtime witnesses are
//! `test_gap_gc_11830_*`; this file covers the sites a Linux gap test cannot
//! reach (`perry/ui` and `perry/system` tables, the FFI manifest, the i18n
//! params) and pins the rest at the IR level.

use crate::testing::temp_slots::{assert_rooted_across, first_call_result};
use crate::{compile_module, CompileOptions};
use perry_api_manifest::NativeAbiType;
use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module, Stmt};

fn object(key: &str) -> Expr {
    Expr::Object(vec![(key.to_string(), Expr::String("pem".to_string()))])
}

fn compile_stmt(stmt: Expr, opts: CompileOptions) -> String {
    let mut module = Module::new("conversion_window_test.ts");
    module.functions.push(Function {
        id: 0,
        name: "build".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![Stmt::Expr(stmt)],
        is_async: false,
        is_generator: false,
        is_strict: true,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
    });
    let ir = String::from_utf8(
        compile_module(
            &module,
            CompileOptions {
                emit_ir_only: true,
                ..opts
            },
        )
        .expect("fixture compiles"),
    )
    .expect("LLVM IR is UTF-8");
    let start = ir
        .find("define double @perry_fn_conversion_window_test_ts__build(")
        .unwrap_or_else(|| panic!("build function was not emitted:\n{ir}"));
    let tail = &ir[start..];
    let end = tail
        .find("\n}\n")
        .unwrap_or_else(|| panic!("build function was not terminated:\n{tail}"));
    tail[..end + 3].to_string()
}

fn native(
    module: &str,
    class: Option<&str>,
    object: Option<Expr>,
    method: &str,
    args: Vec<Expr>,
) -> Expr {
    Expr::NativeMethodCall {
        module: module.to_string(),
        class_name: class.map(str::to_string),
        object: object.map(Box::new),
        method: method.to_string(),
        args,
    }
}

/// The first conversion's pointer must be parked and re-read by the consumer.
fn assert_first_pointer_rooted(ir: &str, converter: &str, consumer: &str, what: &str) {
    let first = first_call_result(ir, converter)
        .unwrap_or_else(|| panic!("{what}: no call to @{converter}:\n{ir}"));
    assert_rooted_across(ir, &first, consumer, what);
}

#[test]
fn ui_table_call_roots_first_string_pointer_across_second_conversion() {
    // U4: `lower_ui_args_by_kind` took `js_get_string_pointer_unified` of each
    // `Str` argument in order.
    let ir = compile_stmt(
        native(
            "perry/system",
            None,
            None,
            "keychainSave",
            vec![object("key"), object("value")],
        ),
        CompileOptions::default(),
    );
    assert_first_pointer_rooted(
        &ir,
        "js_get_string_pointer_unified",
        "perry_system_keychain_save",
        "perry/system keychainSave first string pointer",
    );
}

#[test]
fn native_table_call_roots_first_string_pointer_across_second_conversion() {
    // U2: `lower_native_module_dispatch` took `js_value_to_str_ptr_for_ffi` of
    // each `StrPtr` argument in order.
    let sig = super::native_module_lookup("net", true, "check", Some("BlockList"))
        .expect("net.BlockList.check resolves");
    let ir = compile_stmt(
        native(
            "net",
            Some("BlockList"),
            Some(object("recv")),
            "check",
            vec![object("address"), object("family")],
        ),
        CompileOptions::default(),
    );
    assert_first_pointer_rooted(
        &ir,
        "js_value_to_str_ptr_for_ffi",
        sig.runtime,
        "native-table first StrPtr",
    );
}

#[test]
fn ffi_manifest_call_roots_first_string_pointer_across_second_conversion() {
    // U3: the FFI manifest `string` parameter took
    // `js_native_abi_check_string_ptr` of each argument in order.
    let ir = compile_stmt(
        Expr::Call {
            callee: Box::new(Expr::ExternFuncRef {
                name: "probe_ffi_strings".to_string(),
                param_types: Vec::new(),
                return_type: Type::Void,
            }),
            args: vec![object("first"), object("second")],
            type_args: Vec::new(),
            byte_offset: 0,
        },
        CompileOptions {
            native_library_functions: vec![(
                "probe_ffi_strings".to_string(),
                vec![NativeAbiType::String, NativeAbiType::String],
                NativeAbiType::Void,
            )],
            ..Default::default()
        },
    );
    assert_first_pointer_rooted(
        &ir,
        "js_native_abi_check_string_ptr",
        "probe_ffi_strings",
        "FFI manifest first string parameter",
    );
}

#[test]
fn i18n_params_are_rooted_and_their_coercions_are_rooted() {
    // U1: the params of `t("{a} and {b}", { a, b })` were registers lowered up
    // front and read in every block of the locale / plural diamond, with
    // `js_string_coerce` (which can collect) between the reads.
    let opts = CompileOptions {
        i18n_table: Some(std::sync::Arc::new((
            vec!["{a} and {b}".to_string()],
            1,
            1,
            vec!["en".to_string()],
            0,
            Vec::new(),
        ))),
        ..Default::default()
    };
    let ir = compile_stmt(
        Expr::I18nString {
            key: "{a} and {b}".to_string(),
            string_idx: 0,
            params: vec![
                ("a".to_string(), Box::new(object("a"))),
                ("b".to_string(), Box::new(object("b"))),
            ],
            plural_forms: Vec::new(),
            plural_param: None,
        },
        opts,
    );
    // The first param reaches its coercion from its slot...
    let first_param = first_call_result(&ir, "js_object_alloc_with_shape")
        .or_else(|| first_call_result(&ir, "js_object_alloc"))
        .unwrap_or_else(|| panic!("the first param must allocate an object:\n{ir}"));
    let boxed = ir
        .lines()
        .find_map(|line| {
            let (register, definition) = line.trim().split_once(" = ")?;
            definition
                .starts_with(&format!("or i64 {first_param}, "))
                .then(|| register.to_string())
        })
        .unwrap_or_else(|| panic!("the first param must be NaN-boxed:\n{ir}"));
    assert_rooted_across(&ir, &boxed, "js_string_coerce", "i18n first param");
    // ...and its coerced string is rooted across the second param's coercion.
    assert_first_pointer_rooted(
        &ir,
        "js_string_coerce",
        "js_string_concat",
        "i18n first coerced placeholder",
    );
}
