//! The inline `push` tier was folded from the receiver's ANNOTATION, so the
//! value behind it may be `null`, a Number or a String. Unless the facts
//! prove the site (or the receiver is an owned literal local), the tier tests
//! that the receiver is a heap pointer before it reads a header behind it, and
//! the slow arms test it before they hand the runtime the receiver's address;
//! the miss is the ordinary `Get` + `Call`.

use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module as HirModule, Param, Stmt};

const GENERIC_PUSH: &str = "call double @js_builtin_proto_generic_call1(";

/// `function pushes(a: number[]) { a.push(7); return a; }`, or with `owned`
/// `let a: number[] = []; a.push(7); return a;` (the owned literal local).
fn push_ir(owned: bool) -> String {
    let mut hir = HirModule::new("apush_receiver_test");
    let arr = Type::Array(Box::new(Type::Number));
    let params = if owned {
        Vec::new()
    } else {
        vec![Param {
            id: 0,
            name: "a".to_string(),
            ty: arr.clone(),
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }]
    };
    let mut body = Vec::new();
    if owned {
        body.push(Stmt::Let {
            id: 0,
            name: "a".to_string(),
            ty: arr,
            mutable: true,
            init: Some(Expr::Array(Vec::new(), None)),
        });
    }
    body.push(Stmt::Expr(Expr::ArrayPush {
        array_id: 0,
        value: Box::new(Expr::Number(7.0)),
        field_writeback: None,
    }));
    body.push(Stmt::Return(Some(Expr::LocalGet(0))));
    hir.functions.push(Function {
        id: 0,
        name: "pushes".to_string(),
        type_params: Vec::new(),
        params,
        return_type: Type::Any,
        body,
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    let opts = crate::CompileOptions {
        emit_ir_only: true,
        ..Default::default()
    };
    let bytes = crate::compile_module(&hir, opts).expect("test module compiles");
    String::from_utf8(bytes).expect("LLVM IR is UTF-8")
}

/// The body of the first block whose label starts with `prefix`.
fn block<'a>(ir: &'a str, prefix: &str) -> &'a str {
    let start = ir
        .find(&format!("\n{prefix}"))
        .unwrap_or_else(|| panic!("no {prefix} block in:\n{ir}"))
        + 1;
    let rest = &ir[start..];
    let end = rest.find("\n\n").unwrap_or(rest.len());
    &rest[..end]
}

#[test]
fn an_annotated_parameter_is_pointer_tested_before_its_header_is_read() {
    let ir = push_ir(false);
    assert!(
        ir.contains("apush.nofwd"),
        "the push must take its inline tier, or this test proves nothing:\n{ir}"
    );
    // The header loads sit in the block the band test admits.
    let hdr = block(&ir, "apush.recv.hdr");
    assert!(
        hdr.contains("load i8"),
        "the header is read only behind the band test:\n{hdr}"
    );
    // The miss takes the forwarded arm, whose push entry first sends a
    // non-pointer to the ordinary call.
    assert!(
        block(&ir, "apush.recv.miss").contains("br label %apush.fwd"),
        "a receiver that is no heap pointer takes the forwarded arm:\n{ir}"
    );
    let other = block(&ir, "apush.recv.other");
    assert!(
        other.contains(GENERIC_PUSH),
        "a receiver that is no heap pointer is the ordinary Get + Call:\n{other}"
    );
}

#[test]
fn an_owned_literal_local_keeps_the_tier_without_a_test() {
    let ir = push_ir(true);
    assert!(
        ir.contains("apush.nofwd"),
        "the push must take its inline tier, or this test proves nothing:\n{ir}"
    );
    assert!(
        !ir.contains("apush.recv."),
        "an owned literal local is an Array by birth: no receiver test:\n{ir}"
    );
}
