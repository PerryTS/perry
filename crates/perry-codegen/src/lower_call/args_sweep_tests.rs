//! IR-level regression coverage for the #11789 call-argument sweep.
//!
//! #11789 was a call that lowered its arguments into bare SSA registers, so an
//! earlier heap argument was a pre-move address by the time a later argument
//! had finished collecting. The sweep that followed it moved every other
//! argument-lowering loop onto `lower_call_args_rooted` /
//! `lower_operands_rooted` / `lower_operand_list_rooted` (all one
//! `RootedGroup`). The runtime witnesses are `test_gap_gc_11789_*`; this file
//! covers the sites a Linux gap test cannot reach (the `perry/ui` and
//! `perry/tui` tables) and pins the rest at the IR level.
//!
//! Every test makes the FIRST operand a freshly allocated object literal and
//! the SECOND a sequence containing another object literal (an operand that
//! `any_operand_may_collect` reports as collecting), then asks
//! [`assert_rooted_across`] to prove the first operand was parked in a rooted
//! slot and that the consuming call read it back out of that slot.

use crate::testing::temp_slots::{assert_rooted_across, first_call_result};
use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module, Stmt};

fn object(key: &str) -> Expr {
    Expr::Object(vec![(key.to_string(), Expr::String("pem".to_string()))])
}

/// An operand that allocates (so it is a collection point) and whose value is
/// the object it ends in.
fn collecting(key: &str) -> Expr {
    Expr::Sequence(vec![object("side"), object(key)])
}

fn compile_stmt(stmt: Expr) -> String {
    let mut module = Module::new("args_sweep_test.ts");
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
                ..Default::default()
            },
        )
        .expect("fixture compiles"),
    )
    .expect("LLVM IR is UTF-8");
    let start = ir
        .find("define double @perry_fn_args_sweep_test_ts__build(")
        .unwrap_or_else(|| panic!("build function was not emitted:\n{ir}"));
    let tail = &ir[start..];
    let end = tail
        .find("\n}\n")
        .unwrap_or_else(|| panic!("build function was not terminated:\n{tail}"));
    tail[..end + 3].to_string()
}

/// The NaN-boxed register of the FIRST object literal the function allocates.
fn first_boxed_object(ir: &str) -> String {
    let raw = first_call_result(ir, "js_object_alloc_with_shape")
        .or_else(|| first_call_result(ir, "js_object_alloc"))
        .unwrap_or_else(|| panic!("the first operand must allocate an object:\n{ir}"));
    ir.lines()
        .find_map(|line| {
            let (register, definition) = line.trim().split_once(" = ")?;
            definition
                .starts_with(&format!("or i64 {raw}, "))
                .then(|| register.to_string())
        })
        .unwrap_or_else(|| panic!("the first allocation must be NaN-boxed:\n{ir}"))
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

fn assert_first_operand_rooted(call: Expr, consumer: &str, what: &str) {
    let ir = compile_stmt(call);
    let first = first_boxed_object(&ir);
    assert_rooted_across(&ir, &first, consumer, what);
}

#[test]
fn perry_system_table_call_roots_first_string_argument() {
    // `lower_perry_ui_table_call` converted each `Str` argument to its raw
    // pointer where it was lowered.
    assert_first_operand_rooted(
        native(
            "perry/system",
            None,
            None,
            "keychainSave",
            vec![object("key"), collecting("value")],
        ),
        "perry_system_keychain_save",
        "perry/system keychainSave first argument",
    );
}

#[test]
fn tui_input_roots_content_across_cursor() {
    assert_first_operand_rooted(
        native(
            "perry/tui",
            None,
            None,
            "Input",
            vec![object("content"), collecting("cursor")],
        ),
        "js_perry_tui_input_at",
        "perry/tui Input content",
    );
}

#[test]
fn tui_styled_text_roots_content_across_style_values() {
    assert_first_operand_rooted(
        native(
            "perry/tui",
            None,
            None,
            "Text",
            vec![
                object("content"),
                Expr::Object(vec![("fg".to_string(), collecting("fg"))]),
            ],
        ),
        "js_perry_tui_text_styled",
        "perry/tui Text content",
    );
}

#[test]
fn ui_button_roots_label_across_handler() {
    assert_first_operand_rooted(
        native(
            "perry/ui",
            None,
            None,
            "Button",
            vec![object("label"), collecting("handler")],
        ),
        "perry_ui_button_create",
        "perry/ui Button label",
    );
}

#[test]
fn console_instance_call_roots_receiver_across_arguments() {
    assert_first_operand_rooted(
        native(
            "console",
            Some("Console"),
            Some(object("recv")),
            "log",
            vec![collecting("arg")],
        ),
        "js_native_call_method",
        "Console instance receiver",
    );
}

#[test]
fn native_instance_method_roots_receiver_across_arguments() {
    let sig = super::native_module_lookup("net", true, "write", Some("Socket"))
        .expect("net.Socket.write resolves");
    assert_first_operand_rooted(
        native(
            "net",
            Some("Socket"),
            Some(object("recv")),
            "write",
            vec![collecting("chunk")],
        ),
        sig.runtime,
        "native-table instance receiver",
    );
}

#[test]
fn fs_native_call_roots_first_path_across_second() {
    assert_first_operand_rooted(
        native(
            "fs",
            None,
            None,
            "renameSync",
            vec![object("from"), collecting("to")],
        ),
        "js_fs_rename_sync",
        "fs.renameSync source",
    );
}

#[test]
fn js_runtime_extern_call_roots_first_argument() {
    assert_first_operand_rooted(
        Expr::Call {
            callee: Box::new(Expr::ExternFuncRef {
                name: "js_probe_sweep".to_string(),
                param_types: Vec::new(),
                return_type: Type::Any,
            }),
            args: vec![object("first"), collecting("second")],
            type_args: Vec::new(),
            byte_offset: 0,
        },
        "js_probe_sweep",
        "direct js_* runtime call first argument",
    );
}

#[test]
fn crypto_timing_safe_equal_roots_first_buffer() {
    assert_first_operand_rooted(
        Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::NativeModuleRef("crypto".to_string())),
                property: "timingSafeEqual".to_string(),
                byte_offset: 0,
            }),
            args: vec![object("a"), collecting("b")],
            type_args: Vec::new(),
            byte_offset: 0,
        },
        "js_crypto_timing_safe_equal",
        "crypto.timingSafeEqual first buffer",
    );
}
