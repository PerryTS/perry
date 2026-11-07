//! A `console.<m>(...)` call binds the runtime's console intrinsic only at a
//! site the whole-program facts prove reads the native member
//! (`ElisionFacts::native_member_calls`, keyed by the callee read's offset).
//! Any other site reads `console.<m>` and calls the value it holds: a store
//! the program makes through a value (a parameter, `globalThis[k]`, code run
//! by `new Function` or indirect eval) may have replaced the member.
//!
//! Each test compiles one module twice, the same HIR with and without the
//! site's proof, so the only input that differs is the proof.

use std::collections::BTreeSet;
use std::sync::Arc;

use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module as HirModule, Stmt};

/// The callee read's offset of the one console call under test.
const SITE: u32 = 41;

fn compile(body: Vec<Stmt>, proven: &[u32]) -> String {
    let mut hir = HirModule::new("console_site_facts");
    hir.functions.push(Function {
        id: 0,
        name: "build".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
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
    let elision = crate::ElisionFacts {
        native_member_calls: proven.iter().copied().collect::<BTreeSet<u32>>(),
        ..Default::default()
    };
    let opts = crate::CompileOptions {
        emit_ir_only: true,
        program_facts: crate::ProgramFactsView {
            elision: Arc::new(elision),
            ..Default::default()
        },
        ..Default::default()
    };
    let bytes = crate::compile_module(&hir, opts).expect("test module compiles");
    String::from_utf8(bytes).expect("LLVM IR is UTF-8")
}

fn console_callee(method: &str) -> Box<Expr> {
    Box::new(Expr::PropertyGet {
        byte_end: 0,
        object: Box::new(Expr::GlobalGet(0)),
        property: method.to_string(),
        byte_offset: SITE,
    })
}

fn console_call(method: &str) -> Vec<Stmt> {
    vec![Stmt::Expr(Expr::Call {
        byte_end: 0,
        callee: console_callee(method),
        args: vec![Expr::String("hello".to_string())],
        type_args: Vec::new(),
        byte_offset: SITE,
    })]
}

fn console_spread_call(method: &str) -> Vec<Stmt> {
    vec![Stmt::Expr(Expr::CallSpread {
        byte_end: 0,
        callee: console_callee(method),
        args: vec![perry_hir::CallArg::Spread(Expr::Array(
            vec![Expr::String("hello".to_string())],
            None,
        ))],
        type_args: Vec::new(),
        byte_offset: SITE,
    })]
}

/// The body of the function `build` lowers to.
fn build_body(ir: &str) -> &str {
    let at = ir
        .lines()
        .position(|l| l.starts_with("define ") && l.contains("build"))
        .expect("the build function is defined");
    let start = ir.lines().take(at).map(|l| l.len() + 1).sum::<usize>();
    let end = ir[start..].find("\n}\n").expect("function body ends") + start;
    &ir[start..end]
}

fn calls(body: &str, callee: &str) -> bool {
    body.lines()
        .any(|l| l.contains("call ") && l.contains(&format!("@{callee}(")))
}

#[test]
fn unproven_console_log_reads_the_member_and_calls_it() {
    let ir = compile(console_call("log"), &[]);
    let body = build_body(&ir);
    assert!(
        calls(body, "js_builtin_namespace_member"),
        "an unproven site reads console.log:\n{body}"
    );
    assert!(
        !body.lines().any(|l| l.contains("call ") && l.contains("@js_console_log")),
        "an unproven site never binds the console intrinsic:\n{body}"
    );
}

#[test]
fn proven_console_log_binds_the_intrinsic() {
    let ir = compile(console_call("log"), &[SITE]);
    let body = build_body(&ir);
    assert!(
        body.lines().any(|l| l.contains("call ") && l.contains("@js_console_log")),
        "a proven site binds the console intrinsic:\n{body}"
    );
    assert!(
        !calls(body, "js_builtin_namespace_member"),
        "a proven site reads no member:\n{body}"
    );
}

#[test]
fn a_proof_for_another_site_does_not_bind_the_intrinsic() {
    let ir = compile(console_call("log"), &[SITE + 1]);
    let body = build_body(&ir);
    assert!(
        !body.lines().any(|l| l.contains("call ") && l.contains("@js_console_log")),
        "only the proven offset binds the intrinsic:\n{body}"
    );
}

#[test]
fn unproven_console_spread_call_is_the_real_call() {
    let unproven = compile(console_spread_call("log"), &[]);
    let body = build_body(&unproven);
    assert!(
        !calls(body, "js_console_log_spread"),
        "an unproven spread site never binds the console intrinsic:\n{body}"
    );
    let proven = compile(console_spread_call("log"), &[SITE]);
    assert!(
        calls(build_body(&proven), "js_console_log_spread"),
        "a proven spread site binds the console intrinsic:\n{}",
        build_body(&proven)
    );
}
