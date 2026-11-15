//! `new Promise(executor)` and `JSON.parse(text, reviver)` hand the runtime the
//! callable as an object address decoded by its tag alone (`unbox_pointer_or_null`):
//! the payload of a POINTER_TAG value, 0 for any other word. The plain mask kept
//! a Number's low 48 bits (a denormal's bits, `1.1`'s mantissa) as an address.

use perry_hir::types::Type;
use perry_hir::{Expr, Module, Stmt};

fn ir_of(m: &Module) -> String {
    String::from_utf8(
        crate::compile_module(
            m,
            crate::CompileOptions {
                emit_ir_only: true,
                is_entry_module: true,
                target: Some("x86_64-unknown-linux-gnu".into()),
                ..Default::default()
            },
        )
        .unwrap(),
    )
    .unwrap()
}

/// The lines of the IR from the start of the instruction block that ends in
/// the call of `callee`, up to that call.
fn lines_before_call<'a>(ir: &'a str, callee: &str) -> Vec<&'a str> {
    let lines: Vec<&str> = ir.lines().collect();
    let at = lines
        .iter()
        .position(|l| l.contains(&format!("@{callee}(")) && l.contains("call "))
        .unwrap_or_else(|| panic!("no call of {callee}:\n{ir}"));
    let start = lines[..at]
        .iter()
        .rposition(|l| l.trim_end().ends_with(':') || l.starts_with("define "))
        .unwrap_or(0);
    lines[start..=at].to_vec()
}

fn assert_decoded_by_tag(ir: &str, callee: &str) {
    let block = lines_before_call(ir, callee);
    let text = block.join("\n");
    assert!(
        text.contains("icmp eq i64") && text.contains(crate::nanbox::POINTER_TAG_I64),
        "the callable's tag is compared with POINTER_TAG before {callee}:\n{text}"
    );
    assert!(
        text.contains("select i1"),
        "a non-pointer word reaches {callee} as 0:\n{text}"
    );
}

fn module_with(name: &str, value: Expr) -> Module {
    let mut m = Module::new(name);
    m.init.push(Stmt::Let {
        id: 0,
        name: "callable_arg".into(),
        ty: Type::Any,
        mutable: true,
        init: Some(Expr::Number(1.1)),
    });
    m.init.push(Stmt::Let {
        id: 1,
        name: "callable_result".into(),
        ty: Type::Any,
        mutable: true,
        init: Some(value),
    });
    m
}

#[test]
fn promise_executor_is_decoded_by_its_tag() {
    let m = module_with(
        "callable_promise",
        Expr::New {
            class_name: "Promise".into(),
            args: vec![Expr::LocalGet(0)],
            type_args: vec![],
            byte_offset: 0,
            cap_args_appended: 0,
            byte_end: 0,
        },
    );
    assert_decoded_by_tag(&ir_of(&m), "js_promise_new_with_executor");
}

#[test]
fn json_reviver_is_decoded_by_its_tag() {
    for (name, value) in [
        (
            "callable_reviver_a",
            Expr::JsonParseWithReviver(
                Box::new(Expr::String("1".into())),
                Box::new(Expr::LocalGet(0)),
            ),
        ),
        (
            "callable_reviver_b",
            Expr::JsonParseReviver {
                text: Box::new(Expr::String("1".into())),
                reviver: Box::new(Expr::LocalGet(0)),
            },
        ),
    ] {
        assert_decoded_by_tag(
            &ir_of(&module_with(name, value)),
            "js_json_parse_with_reviver",
        );
    }
}
