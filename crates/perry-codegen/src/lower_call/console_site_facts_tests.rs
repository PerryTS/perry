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
    compile_with_site(body, proven, "", 0)
}

fn compile_with_site(body: Vec<Stmt>, proven: &[u32], key: &str, flags: u8) -> String {
    let mut hir = HirModule::new("console_site_facts");
    hir.functions.push(Function {
        byte_offset: 0,
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
        method_sites: [(
            (perry_hir::folded_call::Site::Callee(SITE), key.to_owned()),
            flags,
        )]
        .into(),
        native_member_calls: proven.iter().copied().collect::<BTreeSet<u32>>(),
        namespace_pair_arms: true,
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
        !body
            .lines()
            .any(|l| l.contains("call ") && l.contains("@js_console_log")),
        "an unproven site never binds the console intrinsic:\n{body}"
    );
}

#[test]
fn proven_console_log_binds_the_intrinsic() {
    let ir = compile(console_call("log"), &[SITE]);
    let body = build_body(&ir);
    assert!(
        body.lines()
            .any(|l| l.contains("call ") && l.contains("@js_console_log")),
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
        !body
            .lines()
            .any(|l| l.contains("call ") && l.contains("@js_console_log")),
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

#[test]
fn native_dispatch_namespace_selection_matches_the_shared_predicate() {
    for (namespace, method, fast, symbol) in [
        (
            "Math",
            "abs",
            Expr::MathAbs(Box::new(Expr::Number(-3.0))),
            "llvm.fabs.f64",
        ),
        (
            "Math",
            "round",
            Expr::MathRound(Box::new(Expr::Number(1.5))),
            "js_math_round",
        ),
        (
            "Number",
            "isInteger",
            Expr::NumberIsInteger(Box::new(Expr::Number(1.0))),
            "js_number_is_integer",
        ),
        (
            "Number",
            "isSafeInteger",
            Expr::NumberIsSafeInteger(Box::new(Expr::Number(1.0))),
            "js_number_is_safe_integer",
        ),
    ] {
        let generic = Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::PropertyGet {
                    object: Box::new(Expr::GlobalThisExpr),
                    property: namespace.into(),
                    byte_offset: 0,
                    byte_end: 0,
                }),
                property: method.into(),
                byte_offset: SITE,
                byte_end: 0,
            }),
            args: vec![Expr::Number(1.0)],
            type_args: vec![],
            byte_offset: SITE,
            byte_end: 0,
        };
        let pair = Expr::Conditional {
            byte_offset: 0,
            condition: Box::new(Expr::NamespacePatched(namespace.into())),
            then_expr: Box::new(generic),
            else_expr: Box::new(fast),
        };
        for proven in [vec![], vec![SITE], vec![SITE + 1]] {
            let selected = perry_hir::namespace_pairs::intrinsic_selected(
                SITE,
                &proven.iter().copied().collect(),
            );
            let ir = compile(vec![Stmt::Expr(pair.clone())], &proven);
            let body = build_body(&ir);
            assert_eq!(
                calls(body, symbol),
                selected,
                "{namespace}.{method}: {body}"
            );
            assert_eq!(
                calls(body, "js_builtin_namespace_member"),
                !selected,
                "{body}"
            );
            if selected {
                assert!(
                    !body
                        .lines()
                        .any(|l| l.contains("call ") && l.contains("js_native_call_method")),
                    "{body}"
                );
            }
        }
    }
}

fn member_call(object: Expr, method: &str, args: Vec<Expr>) -> Vec<Stmt> {
    vec![Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(object),
            property: method.into(),
            byte_offset: SITE,
            byte_end: SITE + 10,
        }),
        args,
        type_args: vec![],
        byte_offset: SITE,
        byte_end: SITE + 10,
    })]
}

#[test]
fn native_dispatch_string_identity_still_uses_a_checked_entry() {
    let ir = compile(
        member_call(
            Expr::String("abc".into()),
            "charAt",
            vec![Expr::Number(0.0)],
        ),
        &[SITE],
    );
    let body = build_body(&ir);
    assert!(
        body.contains("@js_string_m_charAt1("),
        "checked slow arm must remain visible: {body}"
    );
}

#[test]
fn native_dispatch_collection_identity_does_not_remove_receiver_guards() {
    for (object, method, args) in [
        (
            Expr::Array(vec![Expr::Number(1.0)], None),
            "fill",
            vec![Expr::Number(3.0)],
        ),
        (Expr::MapNew, "clear", vec![]),
    ] {
        let ir = compile(member_call(object, method, args), &[SITE]);
        let body = build_body(&ir);
        assert!(
            body.contains("ownoverride.own"),
            "NativeMember alone must not remove the own-method fallback: {body}"
        );
    }
}

#[test]
fn native_dispatch_named_now_is_not_the_receiverless_perf_intrinsic() {
    let ir = compile(
        member_call(
            Expr::PropertyGet {
                object: Box::new(Expr::GlobalGet(0)),
                property: "performance".into(),
                byte_offset: 0,
                byte_end: 0,
            },
            "now",
            vec![],
        ),
        &[],
    );
    let body = build_body(&ir);
    assert!(
        !calls(body, "js_performance_now"),
        "ordinary member call cannot borrow the NativeMethodCall certificate: {body}"
    );
}

#[test]
fn native_dispatch_certificate_selects_direct_entries() {
    use perry_hir::folded_call::{SITE_DIRECT_MAP, SITE_DIRECT_STRING, SITE_MAP};
    let string = compile_with_site(
        member_call(Expr::String("abc".into()), "trim", vec![]),
        &[SITE],
        "trim",
        SITE_DIRECT_STRING,
    );
    assert!(
        calls(build_body(&string), "js_string_trim"),
        "{}",
        build_body(&string)
    );
    let map = Expr::PropertyGet {
        object: Box::new(Expr::GlobalGet(0)),
        property: "table".into(),
        byte_offset: 0,
        byte_end: 0,
    };
    let ir = compile_with_site(
        member_call(map, "clear", vec![]),
        &[SITE],
        "clear",
        SITE_MAP | SITE_DIRECT_MAP,
    );
    assert!(
        calls(build_body(&ir), "js_map_clear"),
        "{}",
        build_body(&ir)
    );
    assert!(!build_body(&ir).contains("ownoverride.own"));
}
