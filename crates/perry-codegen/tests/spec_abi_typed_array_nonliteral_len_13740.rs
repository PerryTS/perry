//! #13740: `s += a[i]` over a spec-ABI `TaPtr` parameter keeps the
//! accumulator's Number-by-construction proof whether or not the typed array's
//! length is an integer literal. The #8619 read proof (a numeric-kind,
//! non-view typed array read is a Number or `undefined`) never uses the
//! length, so `new Int32Array(1 << 20)` must lower like
//! `new Int32Array(1048576)`.

use perry_codegen::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{BinaryOp, Expr, Function, Module, Param, Stmt, TYPED_ARRAY_KIND_INT32};

const ARR: u32 = 100;
const N: u32 = 101;
const S: u32 = 102;
const I: u32 = 103;

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

fn local(id: u32) -> Box<Expr> {
    Box::new(Expr::LocalGet(id))
}

/// `function sum(a, n) { let s = 0; for (let i = 0; i < n; i = i + 1)
/// s = s + a[i]; return s; }`. The `i = i + 1` step keeps the loop off the
/// canonical `i++` form that indexed-owner loop versioning serves on its own.
fn sum_function() -> Function {
    let read = Expr::IndexGet {
        element_offset: 0,
        byte_offset: 0,
        byte_end: 0,
        object: local(ARR),
        index: local(I),
    };
    Function {
        id: 7,
        name: "sum".to_string(),
        type_params: Vec::new(),
        params: vec![param(ARR, "a"), param(N, "n")],
        return_type: Type::Number,
        body: vec![
            Stmt::Let {
                id: S,
                name: "s".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Integer(0)),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: I,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: perry_hir::CompareOp::Lt,
                    left: local(I),
                    right: local(N),
                }),
                update: Some(Expr::LocalSet(
                    I,
                    Box::new(Expr::Binary {
                        op: BinaryOp::Add,
                        left: local(I),
                        right: Box::new(Expr::Integer(1)),
                    }),
                )),
                body: vec![Stmt::Expr(Expr::LocalSet(
                    S,
                    Box::new(Expr::Binary {
                        op: BinaryOp::Add,
                        left: local(S),
                        right: Box::new(read),
                    }),
                ))],
            },
            Stmt::Return(Some(Expr::LocalGet(S))),
        ],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }
}

fn module_with_len(name: &str, len: Expr) -> Module {
    let mut module = Module::new(name);
    module.functions.push(sum_function());
    module.init = vec![
        Stmt::Let {
            id: 1,
            name: "a".to_string(),
            ty: Type::Named("Int32Array".to_string()),
            mutable: false,
            init: Some(Expr::TypedArrayNew {
                kind: TYPED_ARRAY_KIND_INT32,
                arg: Some(Box::new(len)),
            }),
        },
        Stmt::Expr(Expr::Call {
            byte_end: 0,
            callee: Box::new(Expr::FuncRef(7)),
            args: vec![Expr::LocalGet(1), Expr::Integer(1 << 20)],
            type_args: Vec::new(),
            byte_offset: 0,
        }),
    ];
    module
}

fn compile(module: &Module) -> String {
    String::from_utf8(
        compile_module(
            module,
            CompileOptions {
                emit_ir_only: true,
                ..CompileOptions::default()
            },
        )
        .unwrap(),
    )
    .unwrap()
}

/// The body of the one specialized `sum$spec_ta…` entry.
fn spec_sum_body<'a>(ir: &'a str, label: &str) -> &'a str {
    let needle = format!("__sum$spec_{label}");
    let start = ir
        .lines()
        .filter(|line| line.starts_with("define ") && line.contains(&needle))
        .map(|line| line.as_ptr() as usize - ir.as_ptr() as usize)
        .next()
        .unwrap_or_else(|| panic!("no specialized `{needle}` entry in IR:\n{ir}"));
    let rest = &ir[start..];
    let end = rest.find("\n}\n").map_or(rest.len(), |index| index + 3);
    &rest[..end]
}

fn assert_number_accumulator(body: &str) {
    assert!(
        !body.contains("js_dynamic_string_or_number_add"),
        "`s = s + a[i]` over a TaPtr parameter must keep `s` a Number by \
         construction, with no string-or-number add fallback:\n{body}"
    );
}

#[test]
fn literal_length_typed_array_param_keeps_number_accumulator() {
    let ir = compile(&module_with_len(
        "spec_ta_len_literal_13740.ts",
        Expr::Integer(1 << 20),
    ));
    assert_number_accumulator(spec_sum_body(&ir, "ta4x1048576_"));
}

#[test]
fn non_literal_length_typed_array_param_keeps_number_accumulator() {
    let ir = compile(&module_with_len(
        "spec_ta_len_shift_13740.ts",
        Expr::Binary {
            op: BinaryOp::Shl,
            left: Box::new(Expr::Integer(1)),
            right: Box::new(Expr::Integer(20)),
        },
    ));
    assert_number_accumulator(spec_sum_body(&ir, "ta4_"));
}
