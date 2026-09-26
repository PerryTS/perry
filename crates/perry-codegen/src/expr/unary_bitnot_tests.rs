//! Runtime-proof boundary for native unary bitwise-NOT lowering.

use perry_hir::types::Type;
use perry_hir::{BinaryOp, Expr, Function, Module, Param, Stmt, UnaryOp};

use crate::temp_root_coverage::main_ir_for as ir_for;
use crate::{compile_module, CompileOptions};

const X: u32 = 1;
const Y: u32 = 2;
const RESULT: u32 = 3;

fn erased(id: u32, name: &str) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: Type::Any,
        mutable: true,
        init: Some(Expr::Undefined),
    }
}

fn result(expr: Expr) -> Stmt {
    Stmt::Let {
        id: RESULT,
        name: "result".to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(expr),
    }
}

fn bitnot(operand: Expr) -> Expr {
    Expr::Unary {
        op: UnaryOp::BitNot,
        operand: Box::new(operand),
    }
}

#[test]
fn double_bitnot_of_coercive_number_result_stays_native() {
    let quotient = Expr::Binary {
        op: BinaryOp::Div,
        left: Box::new(Expr::LocalGet(X)),
        right: Box::new(Expr::Integer(32)),
    };
    let ir = ir_for(
        "native_double_bitnot",
        vec![erased(X, "x"), result(bitnot(bitnot(quotient)))],
    );

    assert!(
        ir.contains("call double @js_dynamic_div("),
        "the erased division must retain ToNumeric and mixed-BigInt behavior:\n{ir}"
    );
    assert!(
        !ir.contains("call double @js_dynamic_bitnot("),
        "a successfully returned division result is necessarily a Number:\n{ir}"
    );
    assert!(
        ir.matches("xor i32").count() >= 2,
        "both bitwise-NOT operators should lower natively:\n{ir}"
    );
}

#[test]
fn erased_direct_bitnot_retains_bigint_dispatch() {
    let ir = ir_for(
        "dynamic_erased_bitnot",
        vec![erased(X, "x"), result(bitnot(Expr::LocalGet(X)))],
    );
    assert!(
        ir.contains("call double @js_dynamic_bitnot("),
        "an erased operand can be a BigInt and must preserve its tag:\n{ir}"
    );
}

#[test]
fn potentially_bigint_binary_result_retains_bitnot_dispatch() {
    let unknown_value = |id| Expr::PropertyGet {
        byte_offset: 0,
        object: Box::new(Expr::LocalGet(id)),
        property: "value".to_string(),
    };
    let dynamic_and = Expr::Binary {
        op: BinaryOp::BitAnd,
        left: Box::new(unknown_value(X)),
        right: Box::new(unknown_value(Y)),
    };
    let ir = ir_for(
        "dynamic_nested_bigint_bitnot",
        vec![erased(X, "x"), erased(Y, "y"), result(bitnot(dynamic_and))],
    );
    assert!(
        ir.contains("call double @js_dynamic_bitand(")
            && ir.contains("call double @js_dynamic_bitnot("),
        "a both-erased bitwise chain may produce BigInt and must stay dynamic:\n{ir}"
    );
}

// ---------------------------------------------------------------------------
// #10512: `~x` inside a native int32 chain is `xor i32 %x, -1`.
// ---------------------------------------------------------------------------

const P: u32 = 10;
const Q: u32 = 11;
const B: u32 = 12;
const D: u32 = 13;

fn bin(op: BinaryOp, left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op,
        left: Box::new(left),
        right: Box::new(right),
    }
}

/// The body of `function f(p: number, q: number) { const b = p | 0;
/// const d = q | 0; return <ret>; }`, as IR. Every fixture shares the module
/// and function names, so two fixtures compare equal when their code does.
fn int32_locals_fn_ir(ret: Expr) -> String {
    let param = |id: u32| Param {
        id,
        name: format!("p{id}"),
        ty: Type::Number,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    };
    let int32_const = |id: u32, from: u32| Stmt::Let {
        id,
        name: format!("l{id}"),
        ty: Type::Number,
        mutable: false,
        init: Some(bin(BinaryOp::BitOr, Expr::LocalGet(from), Expr::Integer(0))),
    };
    let mut module = Module::new("bitnot_chain");
    module.init.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(1)),
        args: vec![Expr::Integer(1), Expr::Integer(2)],
        type_args: Vec::new(),
        byte_offset: 0,
    }));
    module.functions.push(Function {
        id: 1,
        name: "f".to_string(),
        type_params: Vec::new(),
        params: vec![param(P), param(Q)],
        return_type: Type::Number,
        body: vec![
            int32_const(B, P),
            int32_const(D, Q),
            Stmt::Return(Some(ret)),
        ],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: true,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    let opts = CompileOptions {
        emit_ir_only: true,
        output_type: "executable".to_string(),
        ..Default::default()
    };
    let ir = String::from_utf8(compile_module(&module, opts).expect("module compiles"))
        .expect("LLVM IR is UTF-8");
    let start = ir
        .match_indices("define ")
        .find(|(index, _)| {
            let line_end = ir[*index..].find('\n').map_or(ir.len(), |o| index + o);
            ir[*index..line_end].contains("__f(")
        })
        .map(|(index, _)| index)
        .unwrap_or_else(|| panic!("missing function f:\n{ir}"));
    let end = ir[start..].find("\n}").map_or(ir.len(), |o| start + o);
    ir[start..end].to_string()
}

/// noble's sha256 `Chi`, `((b & d) ^ (~b & d)) | 0`, with `~b` spelled either
/// way. Over an int32 `b` the two spellings are the same operation, so they
/// must be the same code: before #10512 `~b` left the native chain through
/// `sitofp`, and every enclosing `&`/`^`/`| 0` re-entered it through the
/// ToInt32 tower — 10x the instructions of the `b ^ -1` loop.
#[test]
fn bitnot_in_an_int32_chain_is_the_same_code_as_xor_minus_one() {
    let chi = |not_b: Expr| {
        bin(
            BinaryOp::BitOr,
            bin(
                BinaryOp::BitXor,
                bin(BinaryOp::BitAnd, Expr::LocalGet(B), Expr::LocalGet(D)),
                bin(BinaryOp::BitAnd, not_b, Expr::LocalGet(D)),
            ),
            Expr::Integer(0),
        )
    };
    let with_not = int32_locals_fn_ir(chi(bitnot(Expr::LocalGet(B))));
    let with_xor = int32_locals_fn_ir(chi(bin(
        BinaryOp::BitXor,
        Expr::LocalGet(B),
        Expr::Integer(-1),
    )));

    assert!(
        !with_not.contains("js_dynamic_bitnot"),
        "an int32 operand is a Number:\n{with_not}"
    );
    assert!(
        with_not.contains(", -1\n"),
        "`~b` should be a native `xor i32 %b, -1`:\n{with_not}"
    );
    assert_eq!(
        with_not, with_xor,
        "`~b` and `b ^ -1` must lower identically inside an int32 chain"
    );
}

/// A bare `~b` result (no enclosing chain) is also computed natively, and
/// only materialized once, at the return.
#[test]
fn bare_bitnot_of_int32_local_is_one_native_xor() {
    let with_not = int32_locals_fn_ir(bitnot(Expr::LocalGet(B)));
    let with_xor = int32_locals_fn_ir(bin(BinaryOp::BitXor, Expr::LocalGet(B), Expr::Integer(-1)));
    assert!(!with_not.contains("js_dynamic_bitnot"), "{with_not}");
    assert_eq!(
        with_not.matches("sitofp").count(),
        with_xor.matches("sitofp").count(),
        "`~b` should box its result exactly as `b ^ -1` does:\n{with_not}\n---\n{with_xor}"
    );
}
