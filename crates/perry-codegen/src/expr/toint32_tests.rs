//! #10897: user-visible ToInt32 is a predicted diamond, not an unconditional
//! tower. `h = (h + o.a) | 0` paid ~25 bit-manipulation ops per evaluation
//! for the exact modular conversion even though every `|v| < 2^63` is exact
//! through one hardware `fptosi`. These pin the shape on emitted IR: the
//! guard, a fast arm with the bare conversion, and the tower confined to the
//! cold exact arm (so NaN, ±Infinity and `|v| >= 2^63` stay spec-exact).
//!
//! A `number` parameter makes `f` specialize, so its body lives in
//! specialized and generic clones behind a dispatcher. Every clone that
//! converts is checked — a clone that kept the bare tower is the regression.

use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{BinaryOp, Expr, Function, Module, Param, Stmt, UnaryOp};

const A: u32 = 1;
const H: u32 = 2;

/// The tower's 52-bit fraction mask — present only where the exact modular
/// conversion is emitted.
const TOWER_FRACTION_MASK: &str = "4503599627370495";

fn number_param() -> Param {
    Param {
        id: A,
        name: "a".to_string(),
        ty: Type::Number,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

/// `function f(a: number) { <body> }`, called once from module init. Returns
/// the IR of every definition of `f` (dispatcher and clones) that performs a
/// ToInt32 conversion.
fn converting_bodies(target: Option<&str>, body: Vec<Stmt>) -> Vec<String> {
    let mut module = Module::new("toint32_fast_path");
    module.init.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(1)),
        args: vec![Expr::Number(1.5)],
        type_args: Vec::new(),
        byte_offset: 0,
    }));
    module.functions.push(Function {
        id: 1,
        name: "f".to_string(),
        type_params: Vec::new(),
        params: vec![number_param()],
        return_type: Type::Number,
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
        target: target.map(str::to_string),
        ..Default::default()
    };
    let ir = String::from_utf8(compile_module(&module, opts).expect("module compiles"))
        .expect("LLVM IR is UTF-8");
    let bodies: Vec<String> = ir
        .match_indices("define ")
        .filter(|(index, _)| {
            let line_end = ir[*index..].find('\n').map_or(ir.len(), |o| index + o);
            let header = &ir[*index..line_end];
            header.contains("__f(") || header.contains("__f$")
        })
        .map(|(start, _)| {
            let end = ir[start..].find("\n}").map_or(ir.len(), |o| start + o);
            ir[start..end].to_string()
        })
        .filter(|body| {
            body.contains("toint32.")
                || body.contains(TOWER_FRACTION_MASK)
                || body.contains("fjcvtzs")
        })
        .collect();
    assert!(!bodies.is_empty(), "no definition of f converts:\n{ir}");
    bodies
}

/// The diamond tests pin a target without FEAT_JSCVT. `None` would mean the
/// HOST, and on an Apple-silicon host that is a JSCVT CPU, where the whole
/// conversion is one `fjcvtzs` and there is no diamond to find (see
/// `jscvt_targets_keep_the_single_instruction_conversion`).
const DIAMOND_TARGET: &str = "x86_64-unknown-linux-gnu";

fn returning(expr: Expr) -> Vec<Stmt> {
    vec![Stmt::Return(Some(expr))]
}

fn bin(op: BinaryOp, left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op,
        left: Box::new(left),
        right: Box::new(right),
    }
}

/// `(label, body)` for every basic block of `func_ir`.
fn blocks(func_ir: &str) -> Vec<(&str, &str)> {
    let mut out = Vec::new();
    let mut label = "entry";
    let mut start = 0;
    let mut offset = 0;
    for line in func_ir.split_inclusive('\n') {
        if !line.starts_with(' ') && line.trim_end().ends_with(':') {
            out.push((label, &func_ir[start..offset]));
            label = line.trim_end().trim_end_matches(':');
            start = offset + line.len();
        }
        offset += line.len();
    }
    out.push((label, &func_ir[start..]));
    out
}

fn fast_blocks(func_ir: &str) -> usize {
    blocks(func_ir)
        .iter()
        .filter(|(label, _)| label.starts_with("toint32.fast"))
        .count()
}

fn assert_guarded_diamond(what: &str, ir: &str) {
    let blocks = blocks(ir);
    let fast: Vec<_> = blocks
        .iter()
        .filter(|(label, _)| label.starts_with("toint32.fast"))
        .collect();
    let exact: Vec<_> = blocks
        .iter()
        .filter(|(label, _)| label.starts_with("toint32.exact"))
        .collect();
    assert!(
        !fast.is_empty() && fast.len() == exact.len(),
        "{what}: ToInt32 must open a fast/exact diamond:\n{ir}"
    );
    assert!(
        ir.contains("fcmp olt double") && ir.contains("0x43E0000000000000"),
        "{what}: the fast arm is guarded by |v| < 2^63 (ordered, so NaN is out):\n{ir}"
    );
    for (label, body) in &fast {
        assert!(
            body.contains("fptosi double") && body.contains("to i64"),
            "{what}: {label} converts through fptosi -> i64:\n{ir}"
        );
        assert!(
            !body.contains(TOWER_FRACTION_MASK),
            "{what}: {label} must not carry the exact tower:\n{ir}"
        );
    }
    for (label, body) in &exact {
        assert!(
            body.contains(TOWER_FRACTION_MASK),
            "{what}: {label} keeps the exact modular tower for NaN/Inf/|v|>=2^63:\n{ir}"
        );
    }
    let tower_blocks = blocks
        .iter()
        .filter(|(_, body)| body.contains(TOWER_FRACTION_MASK))
        .count();
    assert_eq!(
        tower_blocks,
        exact.len(),
        "{what}: the tower runs only on the cold exact arm:\n{ir}"
    );
    assert!(
        ir.contains("phi i32"),
        "{what}: the two arms merge into one i32:\n{ir}"
    );
}

#[test]
fn bitwise_or_zero_of_a_double_takes_the_guarded_fast_path() {
    let body = returning(bin(BinaryOp::BitOr, Expr::LocalGet(A), Expr::Integer(0)));
    for ir in converting_bodies(Some(DIAMOND_TARGET), body) {
        assert_guarded_diamond("a | 0", &ir);
    }
}

#[test]
fn every_toint32_operator_shares_the_diamond() {
    for op in [
        BinaryOp::BitAnd,
        BinaryOp::BitXor,
        BinaryOp::Shl,
        BinaryOp::Shr,
        BinaryOp::UShr,
    ] {
        let body = returning(bin(op, Expr::LocalGet(A), Expr::Integer(3)));
        for ir in converting_bodies(Some(DIAMOND_TARGET), body) {
            assert_guarded_diamond(&format!("a {op:?} 3"), &ir);
        }
    }
    let body = returning(Expr::Unary {
        op: UnaryOp::BitNot,
        operand: Box::new(Expr::LocalGet(A)),
    });
    for ir in converting_bodies(Some(DIAMOND_TARGET), body) {
        assert_guarded_diamond("~a", &ir);
    }
}

#[test]
fn jscvt_targets_keep_the_single_instruction_conversion() {
    let body = returning(bin(BinaryOp::BitOr, Expr::LocalGet(A), Expr::Integer(0)));
    for ir in converting_bodies(Some("arm64-apple-macosx15.0.0"), body) {
        assert!(
            ir.contains("@llvm.aarch64.fjcvtzs("),
            "fjcvtzs is the whole conversion on FEAT_JSCVT targets:\n{ir}"
        );
        assert!(
            !ir.contains("toint32.fast") && !ir.contains(TOWER_FRACTION_MASK),
            "nothing to branch around when one instruction is exact:\n{ir}"
        );
    }
}

#[test]
fn a_toint32_result_enters_an_i32_slot_without_a_second_conversion() {
    // `let h = 0; h = (h + a) | 0; return h;` — `h` is a canonical i32
    // local, so the `| 0` result (an i32 widened to double as the
    // expression's value) is converted back for the slot store. That second
    // conversion is the identity and must not open a second diamond.
    let body = vec![
        Stmt::Let {
            id: H,
            name: "h".to_string(),
            ty: Type::Number,
            mutable: true,
            init: Some(Expr::Integer(0)),
        },
        Stmt::Expr(Expr::LocalSet(
            H,
            Box::new(bin(
                BinaryOp::BitOr,
                bin(BinaryOp::Add, Expr::LocalGet(H), Expr::LocalGet(A)),
                Expr::Integer(0),
            )),
        )),
        Stmt::Return(Some(Expr::LocalGet(H))),
    ];
    for ir in converting_bodies(Some(DIAMOND_TARGET), body) {
        assert!(
            ir.contains("alloca i32"),
            "h is a canonical i32 local:\n{ir}"
        );
        assert_guarded_diamond("(h + a) | 0", &ir);
        assert_eq!(
            fast_blocks(&ir),
            1,
            "only `| 0` converts; the slot store reuses its i32:\n{ir}"
        );
    }
}
