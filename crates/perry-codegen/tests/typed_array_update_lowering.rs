//! `ta[i]--` / `ta[i]++` on a typed-array receiver lowers to the guarded inline
//! element read and store the explicit `ta[i] = ta[i] - 1` uses, not to the
//! generic four-call update (`js_dyn_index_get`, `js_to_numeric`,
//! `js_numeric_step`, `js_put_value_set`). On fannkuch's `count[r]--` the
//! generic chain was ~21% of the program's instructions.
//!
//! The `any` receiver below is the control: it must keep the generic chain, so
//! the body extraction and the call spellings are proven to see it.

use perry_codegen::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{BinaryOp, Expr, Function, Module, Param, Stmt};

fn param(id: u32, name: &str, ty: Type) -> Param {
    Param {
        id,
        name: name.to_string(),
        ty,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

/// `function bump(i: number) { const a = <receiver>; return a[i | 0]<op><op> }`.
/// `receiver` is `(declared type, initializer)`.
fn module_with_update(receiver: (Type, Expr), op: BinaryOp, prefix: bool) -> Module {
    let mut module = Module::new("typed_array_update");
    let (ty, init) = receiver;
    let update = Expr::IndexUpdate {
        object: Box::new(Expr::LocalGet(2)),
        // `i | 0`: a Number in every clone, including the generic one, whose
        // parameters carry no type.
        index: Box::new(Expr::Binary {
            op: BinaryOp::BitOr,
            left: Box::new(Expr::LocalGet(1)),
            right: Box::new(Expr::Integer(0)),
        }),
        op,
        prefix,
        strict: true,
    };
    module.functions.push(Function {
        id: 1,
        name: "bump".to_string(),
        type_params: Vec::new(),
        params: vec![param(1, "i", Type::Number)],
        return_type: Type::Number,
        body: vec![
            Stmt::Let {
                id: 2,
                name: "a".to_string(),
                ty,
                mutable: false,
                init: Some(init),
            },
            Stmt::Return(Some(update)),
        ],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    module
}

/// Every emitted body of `bump` (public entry and any clone), concatenated.
fn bump_bodies(receiver: (Type, Expr), op: BinaryOp, prefix: bool) -> String {
    let options = CompileOptions {
        emit_ir_only: true,
        ..Default::default()
    };
    let ir = String::from_utf8(
        compile_module(&module_with_update(receiver, op, prefix), options).unwrap(),
    )
    .unwrap();
    let mut out = Vec::new();
    let mut inside = false;
    for line in ir.lines() {
        if line.starts_with("define ") {
            inside = line.contains("@perry_fn_typed_array_update__bump");
        }
        if inside {
            out.push(line);
            if line == "}" {
                inside = false;
            }
        }
    }
    assert!(!out.is_empty(), "bump is present:\n{ir}");
    out.join("\n")
}

fn calls(body: &str, callee: &str) -> usize {
    let needle = format!("@{callee}(");
    body.lines()
        .filter(|l| l.contains("call ") && l.contains(&needle))
        .count()
}

fn typed_array(class_name: &str, kind: u8) -> (Type, Expr) {
    (
        Type::Named(class_name.to_string()),
        Expr::TypedArrayNew {
            kind,
            arg: Some(Box::new(Expr::Integer(8))),
        },
    )
}

#[test]
fn typed_array_update_takes_the_inline_element_path() {
    use perry_hir::*;
    for (class_name, kind) in [
        ("Int8Array", TYPED_ARRAY_KIND_INT8),
        ("Uint8ClampedArray", TYPED_ARRAY_KIND_UINT8_CLAMPED),
        ("Int16Array", TYPED_ARRAY_KIND_INT16),
        ("Uint16Array", TYPED_ARRAY_KIND_UINT16),
        ("Int32Array", TYPED_ARRAY_KIND_INT32),
        ("Uint32Array", TYPED_ARRAY_KIND_UINT32),
        ("Float16Array", TYPED_ARRAY_KIND_FLOAT16),
        ("Float32Array", TYPED_ARRAY_KIND_FLOAT32),
        ("Float64Array", TYPED_ARRAY_KIND_FLOAT64),
    ] {
        for (op, prefix) in [
            (BinaryOp::Sub, false),
            (BinaryOp::Add, false),
            (BinaryOp::Sub, true),
            (BinaryOp::Add, true),
        ] {
            let body = bump_bodies(typed_array(class_name, kind), op, prefix);
            let what = format!("{class_name} {op:?} prefix={prefix}");
            assert_eq!(calls(&body, "js_dyn_index_get"), 0, "{what}:\n{body}");
            assert_eq!(calls(&body, "js_put_value_set"), 0, "{what}:\n{body}");
            // The Number arm steps inline.
            let step = if op == BinaryOp::Sub {
                "fsub double"
            } else {
                "fadd double"
            };
            assert!(
                body.lines().any(|l| l.contains(step) && l.contains("1.0")),
                "{what}: no inline IEEE step:\n{body}"
            );
            // Anything else (BigInt, a lying annotation) keeps ToNumeric and
            // the numeric step off the Number arm, and writes through the
            // complete [[Set]].
            assert!(calls(&body, "js_to_numeric") >= 1, "{what}:\n{body}");
            assert!(calls(&body, "js_numeric_step") >= 1, "{what}:\n{body}");
            assert!(
                calls(&body, "js_dyn_index_set_strict") >= 1,
                "{what}:\n{body}"
            );
        }
    }
}

#[test]
fn untyped_receiver_keeps_the_generic_update() {
    let receiver = (Type::Any, Expr::Array(vec![Expr::Integer(1)]));
    let body = bump_bodies(receiver, BinaryOp::Sub, false);
    assert!(calls(&body, "js_dyn_index_get") >= 1, "{body}");
    assert!(calls(&body, "js_put_value_set") >= 1, "{body}");
}
