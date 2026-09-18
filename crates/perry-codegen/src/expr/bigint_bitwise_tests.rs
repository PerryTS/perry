//! #10418: `&` `|` `^` `<<` `>>` over operands that may be BigInts compute a
//! BigInt. Codegen used to assume every bitwise result is an int32 Number:
//! `const x = a & b` took an int32 slot (the BigInt read back as `0`) and
//! `Number(a & b)` elided its coercion (the BigInt passed through). Each
//! assertion is paired with the Number shape that must keep its fast path.

use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{BinaryOp, Expr, Function, Module, Param, Stmt};

const A: u32 = 1;
const B: u32 = 2;
const X: u32 = 3;

fn param(id: u32, ty: Type) -> Param {
    Param {
        id,
        name: format!("p{id}"),
        ty,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

fn bin(op: BinaryOp, left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op,
        left: Box::new(left),
        right: Box::new(right),
    }
}

/// `function f(a, b) { <body> }`, called once from module init, as IR.
fn function_ir(name: &str, params: Vec<Param>, body: Vec<Stmt>) -> String {
    let mut module = Module::new(name);
    module.init.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(1)),
        args: params.iter().map(|_| Expr::Undefined).collect(),
        type_args: Vec::new(),
        byte_offset: 0,
    }));
    module.functions.push(Function {
        id: 1,
        name: "f".to_string(),
        type_params: Vec::new(),
        params,
        return_type: Type::Any,
        body,
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

/// `const x = <init>; return x;`
fn const_then_return(init: Expr, ty: Type) -> Vec<Stmt> {
    vec![
        Stmt::Let {
            id: X,
            name: "x".to_string(),
            ty,
            mutable: false,
            init: Some(init),
        },
        Stmt::Return(Some(Expr::LocalGet(X))),
    ]
}

#[test]
fn possibly_bigint_bitwise_const_takes_no_int32_slot() {
    for op in [
        BinaryOp::BitAnd,
        BinaryOp::BitOr,
        BinaryOp::BitXor,
        BinaryOp::Shl,
        BinaryOp::Shr,
    ] {
        let untyped = function_ir(
            "bigint_bitwise_untyped",
            vec![param(A, Type::Any), param(B, Type::Any)],
            const_then_return(bin(op, Expr::LocalGet(A), Expr::LocalGet(B)), Type::Any),
        );
        assert!(
            !untyped.contains("alloca i32"),
            "{op:?} over untyped operands may be a BigInt, so `x` needs a boxed slot:\n{untyped}"
        );

        let typed = function_ir(
            "bigint_bitwise_typed",
            vec![param(A, Type::BigInt), param(B, Type::BigInt)],
            const_then_return(bin(op, Expr::LocalGet(A), Expr::LocalGet(B)), Type::BigInt),
        );
        assert!(
            !typed.contains("alloca i32"),
            "{op:?} over `bigint` operands is a BigInt:\n{typed}"
        );

        let masked = function_ir(
            "number_bitwise_masked",
            vec![param(A, Type::Any), param(B, Type::Any)],
            const_then_return(bin(op, Expr::LocalGet(A), Expr::Integer(3)), Type::Number),
        );
        assert!(
            masked.contains("alloca i32"),
            "{op:?} with a Number operand is an int32 and keeps its slot:\n{masked}"
        );
    }
}

#[test]
fn number_of_possibly_bigint_bitwise_result_keeps_its_coercion() {
    let number_of = |right: Expr| {
        function_ir(
            "number_of_bitwise",
            vec![param(A, Type::Any), param(B, Type::Any)],
            vec![Stmt::Return(Some(Expr::NumberCoerce(Box::new(bin(
                BinaryOp::BitAnd,
                Expr::LocalGet(A),
                right,
            )))))],
        )
    };
    let unknown = number_of(Expr::LocalGet(B));
    assert!(
        unknown.contains("call double @js_number_coerce("),
        "`Number(a & b)` must convert a BigInt result:\n{unknown}"
    );
    let masked = number_of(Expr::Integer(255));
    assert!(
        !masked.contains("call double @js_number_coerce("),
        "`Number(a & 255)` is already a Number:\n{masked}"
    );
}

/// With the int32 slot gone, an unproven operand keeps the Number case inline
/// behind a tag test; a BigInt (or any non-Number) takes the helper arm.
#[test]
fn unproven_bitwise_operands_take_a_guarded_int32_arm() {
    for (op, helper, native) in [
        (BinaryOp::BitAnd, "js_dynamic_bitand", "and i32"),
        (BinaryOp::BitOr, "js_dynamic_bitor", "or i32"),
        (BinaryOp::BitXor, "js_dynamic_bitxor", "xor i32"),
        (BinaryOp::Shl, "js_dynamic_shl", "shl i32"),
        (BinaryOp::Shr, "js_dynamic_shr", "ashr i32"),
        (BinaryOp::UShr, "js_dynamic_ushr", "lshr i32"),
    ] {
        let ir = function_ir(
            "guarded_bitwise",
            vec![param(A, Type::Any), param(B, Type::Any)],
            vec![Stmt::Return(Some(bin(
                op,
                Expr::LocalGet(A),
                Expr::LocalGet(B),
            )))],
        );
        assert!(
            ir.contains("guarded_arith.numeric"),
            "{op:?} over unproven operands should test for Numbers:\n{ir}"
        );
        assert!(
            ir.contains(&format!("call double @{helper}(")),
            "{op:?} must keep the BigInt-aware helper for non-Numbers:\n{ir}"
        );
        assert!(
            ir.contains(native),
            "{op:?} should compute Numbers inline:\n{ir}"
        );
        if matches!(op, BinaryOp::Shl | BinaryOp::Shr | BinaryOp::UShr) {
            assert!(
                ir.contains(", 31\n"),
                "{op:?} must mask its shift count to 5 bits:\n{ir}"
            );
        }
    }
}
