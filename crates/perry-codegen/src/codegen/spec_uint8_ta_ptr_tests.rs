//! A `Uint8Array` argument reaches its callee as a `TaPtr`, like every other
//! numeric typed-array kind.
//!
//! `new Uint8Array(n)` lowers to `Expr::Uint8ArrayNew`, not
//! `Expr::TypedArrayNew`, so the call-site pre-pass never proved its binding:
//! the callee kept the boxed ABI and `s = s + a[i]` went through
//! `js_dynamic_string_or_number_add` on every iteration. The runtime object is
//! the same non-view byte store either way (`GC_TYPE_BUFFER_UINT8ARRAY`).

use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{BinaryOp, CompareOp, Expr, Function, Module, Param, Stmt};

const ARR: u32 = 10;
const I: u32 = 11;
const S: u32 = 12;
const TABLE: u32 = 20;

fn local(id: u32, name: &str) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: Type::Number,
        mutable: true,
        init: Some(Expr::Integer(0)),
    }
}

fn add(left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(left),
        right: Box::new(right),
    }
}

/// `function sum(a: Uint8Array) { let i = 0; let s = 0; while (i < 1024) { s = s + a[i]; i = i + 1; } return s; }`
fn sum_function() -> Function {
    Function {
        id: 1,
        name: "sum".to_string(),
        type_params: Vec::new(),
        params: vec![Param {
            id: ARR,
            name: "a".to_string(),
            ty: Type::Named("Uint8Array".to_string()),
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Number,
        body: vec![
            local(I, "i"),
            local(S, "s"),
            Stmt::While {
                condition: Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(I)),
                    right: Box::new(Expr::Integer(1024)),
                },
                body: vec![
                    Stmt::Expr(Expr::LocalSet(
                        S,
                        Box::new(add(
                            Expr::LocalGet(S),
                            Expr::Uint8ArrayGet {
                                array: Box::new(Expr::LocalGet(ARR)),
                                index: Box::new(Expr::LocalGet(I)),
                            },
                        )),
                    )),
                    Stmt::Expr(Expr::LocalSet(
                        I,
                        Box::new(add(Expr::LocalGet(I), Expr::Integer(1))),
                    )),
                ],
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

/// Compile `const table = new Uint8Array(<length>); sum(table);` and return
/// the body of the function whose `define` line contains `marker`.
fn specialized_sum_ir(file: &str, length: Expr, marker: &str) -> String {
    let mut module = Module::new(file);
    module.functions.push(sum_function());
    module.init.extend([
        Stmt::Let {
            id: TABLE,
            name: "table".to_string(),
            ty: Type::Named("Uint8Array".to_string()),
            mutable: false,
            init: Some(Expr::Uint8ArrayNew(Some(Box::new(length)))),
        },
        Stmt::Expr(Expr::Call {
            byte_end: 0,
            callee: Box::new(Expr::FuncRef(1)),
            args: vec![Expr::LocalGet(TABLE)],
            type_args: Vec::new(),
            byte_offset: 0,
        }),
    ]);
    let opts = CompileOptions {
        emit_ir_only: true,
        output_type: "executable".to_string(),
        ..Default::default()
    };
    let ir = String::from_utf8(compile_module(&module, opts).expect("module compiles"))
        .expect("LLVM IR is UTF-8");
    let start = ir
        .lines()
        .position(|line| line.starts_with("define ") && line.contains(marker))
        .unwrap_or_else(|| panic!("missing specialized clone {marker}:\n{ir}"));
    ir.lines()
        .skip(start)
        .take_while(|line| *line != "}")
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_reads_are_raw_bytes_into_a_double(clone: &str) {
    assert!(
        !clone.contains("js_dynamic_string_or_number_add"),
        "`s = s + a[i]` over a Uint8Array TaPtr is a Number by construction; \
         the add must not keep a dynamic arm:\n{clone}"
    );
    for helper in [
        "js_typed_array_index_get_dynamic",
        "js_uint8array_index_get_value",
    ] {
        assert!(
            !clone.contains(helper),
            "a TaPtr read is a bounds-checked load off the hoisted data \
             pointer, with no receiver guard and no `{helper}` arm:\n{clone}"
        );
    }
    assert!(
        clone.contains("load i8"),
        "the element read must be a raw byte load:\n{clone}"
    );
    assert!(
        clone.contains("alloca double"),
        "the accumulator must live in a raw double slot:\n{clone}"
    );
}

#[test]
fn uint8array_with_non_literal_length_reaches_the_ta_ptr_clone() {
    // `new Uint8Array(1 << 20)`
    let clone = specialized_sum_ir(
        "u8_shift_length.ts",
        Expr::Binary {
            op: BinaryOp::Shl,
            left: Box::new(Expr::Integer(1)),
            right: Box::new(Expr::Integer(20)),
        },
        "sum$spec_ta1(",
    );
    assert_reads_are_raw_bytes_into_a_double(&clone);
}

#[test]
fn uint8array_with_literal_length_reaches_the_ta_ptr_clone() {
    // `new Uint8Array(1048576)`
    let clone = specialized_sum_ir(
        "u8_literal_length.ts",
        Expr::Integer(1 << 20),
        "sum$spec_ta1x1048576(",
    );
    assert_reads_are_raw_bytes_into_a_double(&clone);
}
