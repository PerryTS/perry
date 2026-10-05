//! #11910: `recv.m(args)` reads `recv.m` before arguments that can run code.
//!
//! A call whose arguments are order-free keeps the fused method site (no
//! lookup half, no `js_method_site_lookup`); a call with an effectful
//! argument emits the lookup half, and the read happens before the argument's
//! call. `recv.m?.(args)` is one split site whose lookup value its guard
//! tests, so `recv.m` is read once.

use crate::compile_module;
use perry_hir::types::Type;
use perry_hir::{CompareOp, Expr, Function, Module, ModuleInitKind, Param, Stmt};

const RECV: u32 = 1;
const G: u32 = 2;
const LOOKUP: &str = "call double @js_method_site_lookup(";
const CALL_SPLIT: &str = "call double @js_method_site_call_split(";

fn param(id: u32, name: &str) -> Param {
    Param {
        id,
        name: name.to_string(),
        ty: Type::Any,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

fn method_call(recv: Expr, args: Vec<Expr>) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(recv),
            property: "m".to_string(),
            byte_offset: 0,
        }),
        args,
        type_args: Vec::new(),
        byte_offset: 0,
    }
}

fn effectful_arg() -> Expr {
    Expr::Call {
        callee: Box::new(Expr::LocalGet(G)),
        args: Vec::new(),
        type_args: Vec::new(),
        byte_offset: 0,
    }
}

/// `function f(o, g) { return <body>; }` in an otherwise empty module.
fn module_with(body: Expr) -> Module {
    let mut m = Module::new("method_order.ts");
    m.functions.push(Function {
        id: 50,
        name: "f".to_string(),
        type_params: Vec::new(),
        params: vec![param(RECV, "o"), param(G, "g")],
        return_type: Type::Any,
        body: vec![Stmt::Return(Some(body))],
        is_async: false,
        is_generator: false,
        is_exported: true,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
        is_strict: true,
    });
    m.init_kind = ModuleInitKind::Eager;
    m
}

fn ir_for(m: Module) -> String {
    String::from_utf8(
        compile_module(&m, crate::temp_root_coverage::entry_opts()).expect("module compiles"),
    )
    .expect("LLVM IR is UTF-8")
}

/// The body of `@perry_fn_..._f`.
fn f_body(ir: &str) -> &str {
    let start = ir
        .match_indices("define ")
        .map(|(i, _)| i)
        .find(|&i| ir[i..].lines().next().is_some_and(|l| l.contains("__f(")))
        .expect("f is defined");
    let rest = &ir[start..];
    &rest[..rest.find("\n}\n").expect("f ends")]
}

#[test]
fn an_order_free_argument_keeps_the_fused_site() {
    let ir = ir_for(module_with(method_call(
        Expr::LocalGet(RECV),
        vec![Expr::Number(1.0), Expr::LocalGet(G)],
    )));
    let f = f_body(&ir);
    assert!(
        f.contains("call double @js_method_site_miss("),
        "the site is emitted:\n{f}"
    );
    assert!(
        !f.contains(LOOKUP),
        "no lookup half for order-free arguments:\n{f}"
    );
    assert!(
        !f.contains(CALL_SPLIT),
        "no split call half for order-free arguments:\n{f}"
    );
}

#[test]
fn an_effectful_argument_runs_after_the_lookup() {
    let ir = ir_for(module_with(method_call(
        Expr::LocalGet(RECV),
        vec![effectful_arg()],
    )));
    let f = f_body(&ir);
    let lookup = f
        .find(LOOKUP)
        .expect("the lookup runs before the arguments");
    let arg_call = f
        .find("call double @js_native_call_value(")
        .or_else(|| f.find("@js_closure_call0("))
        .expect("the argument's call is emitted");
    assert!(lookup < arg_call, "the read precedes the argument:\n{f}");
    let call = f
        .find(CALL_SPLIT)
        .expect("a read value or a by-name answer is called after the arguments");
    assert!(arg_call < call, "the call follows the argument:\n{f}");
}

#[test]
fn an_optional_call_reads_the_method_once() {
    // `o.m?.(1)` as the optional-chain lowering builds it.
    let guard = Expr::Compare {
        op: CompareOp::LooseEq,
        left: Box::new(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(RECV)),
            property: "m".to_string(),
            byte_offset: 0,
        }),
        right: Box::new(Expr::Null),
    };
    let ir = ir_for(module_with(Expr::Conditional {
        condition: Box::new(guard),
        then_expr: Box::new(Expr::Undefined),
        else_expr: Box::new(method_call(Expr::LocalGet(RECV), vec![Expr::Number(1.0)])),
    }));
    let f = f_body(&ir);
    assert_eq!(f.matches(LOOKUP).count(), 1, "one split site:\n{f}");
    assert!(
        f.contains("optcall.short"),
        "the guard tests the lookup value:\n{f}"
    );
    // The guard's own read sits only on the by-name path (`msite.opt_named`),
    // after the lookup.
    let named = f.find("msite.opt_named").expect("by-name guard block");
    assert!(f.find(LOOKUP).unwrap() < named, "lookup first:\n{f}");
}
