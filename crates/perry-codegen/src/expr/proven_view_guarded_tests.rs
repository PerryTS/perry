//! Emission pins for the guarded proven-view tier
//! (`expr/proven_view_guarded.rs`) and for `new TA(n)` ownership through a
//! non-Object length. The behavioural half is
//! `test-files/test_gap_typed_array_guarded_view_keys.ts` and
//! `test_gap_typed_array_length_ownership.ts`, compared against node.

use super::class_field_barrier_tests::block_body;
use crate::testing::root_slots::function_slice;
use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{BinaryOp, Expr, Function, Module, Param, Stmt};

fn param(id: u32, ty: Type) -> Param {
    Param {
        id,
        name: format!("p{id}"),
        ty,
        default: None,
        decorators: vec![],
        is_rest: false,
        arguments_object: None,
    }
}

/// Every `probe` function (public entry and clones), concatenated.
fn probe_ir(name: &str, params: Vec<Param>, body: Vec<Stmt>) -> String {
    probe_functions(name, params, body)
        .into_iter()
        .map(|(_, body)| body)
        .collect::<Vec<_>>()
        .join("\n")
}

/// `(symbol, body)` for the public entry and every clone of `probe`.
fn probe_functions(name: &str, params: Vec<Param>, body: Vec<Stmt>) -> Vec<(String, String)> {
    let mut module = Module::new(name);
    module.functions.push(Function {
        id: 1,
        name: "probe".into(),
        type_params: vec![],
        params,
        return_type: Type::Any,
        body,
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: true,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    });
    let ir = String::from_utf8(
        compile_module(
            &module,
            CompileOptions {
                emit_ir_only: true,
                is_entry_module: false,
                ..CompileOptions::default()
            },
        )
        .expect("compile guarded-view probe"),
    )
    .unwrap();
    let prefix = format!("perry_fn_{}__probe", name.replace('.', "_"));
    let mut out = Vec::new();
    for line in ir.lines() {
        if line.starts_with("define ") && line.contains(&format!("@{prefix}")) {
            let name_start = line.find('@').unwrap() + 1;
            let name_end = line[name_start..].find('(').unwrap() + name_start;
            let symbol = &line[name_start..name_end];
            out.push((symbol.to_string(), function_slice(&ir, symbol).to_string()));
        }
    }
    assert!(!out.is_empty(), "no probe function in:\n{ir}");
    out
}

fn let_(id: u32, mutable: bool, ty: Type, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: format!("v{id}"),
        ty,
        mutable,
        init: Some(init),
    }
}

fn int32_new(len: Expr) -> Expr {
    Expr::TypedArrayNew {
        kind: perry_hir::TYPED_ARRAY_KIND_INT32,
        arg: Some(Box::new(len)),
    }
}

fn get(view: u32, index: Expr) -> Expr {
    Expr::IndexGet {
        object: Box::new(Expr::LocalGet(view)),
        index: Box::new(index),
    }
}

fn sub1(e: Expr) -> Expr {
    Expr::Binary {
        op: BinaryOp::Sub,
        left: Box::new(e),
        right: Box::new(Expr::Integer(1)),
    }
}

/// The fannkuch shape: `let k = a[0]; a[k] = a[k - 1]; return a[k]`. `k` is
/// a Number or `undefined`, so it is not a Number, but it is a numeric key:
/// every access stays on the owned view behind one run-time index test.
fn swap_body(view_init: Expr, extra: Vec<Stmt>) -> Vec<Stmt> {
    let mut body = extra;
    body.extend([
        let_(10, false, Type::Named("Int32Array".into()), view_init),
        let_(11, true, Type::Number, get(10, Expr::Integer(0))),
        Stmt::Expr(Expr::IndexSet {
            object: Box::new(Expr::LocalGet(10)),
            index: Box::new(Expr::LocalGet(11)),
            value: Box::new(get(10, sub1(Expr::LocalGet(11)))),
        }),
        Stmt::Return(Some(get(10, Expr::LocalGet(11)))),
    ]);
    body
}

#[test]
fn numeric_key_reads_and_stores_stay_on_the_owned_view() {
    let ir = probe_ir(
        "gview_swap",
        vec![],
        swap_body(int32_new(Expr::Integer(8)), vec![]),
    );
    let load =
        block_body(&ir, "pview.gget.load").unwrap_or_else(|| panic!("no guarded load arm:\n{ir}"));
    assert!(
        load.contains("load i32"),
        "the hit arm is a bare element load:\n{load}"
    );
    let slow =
        block_body(&ir, "pview.gget.slow").unwrap_or_else(|| panic!("no guarded load exit:\n{ir}"));
    assert!(slow.contains("@js_dyn_index_get("), "{slow}");
    let store_at = ir
        .find("pview.gset.store")
        .unwrap_or_else(|| panic!("no guarded store arm:\n{ir}"));
    // The arm converts with ToInt32 (its own blocks) and then stores.
    assert!(ir[store_at..].contains("store i32"), "{ir}");
    let slow = block_body(&ir, "pview.gset.slow")
        .unwrap_or_else(|| panic!("no guarded store exit:\n{ir}"));
    assert!(slow.contains("@js_dyn_index_set_strict("), "{slow}");
    for gone in [
        "@js_typed_array_index_set_dynamic(",
        "@js_typed_array_index_get_dynamic(",
        "tav.set.fast",
    ] {
        assert!(!ir.contains(gone), "{gone} must be gone:\n{ir}");
    }
}

/// A key that may be a string takes no guarded arm: `"buffer"` would rebind
/// the array's storage behind the cached pointer.
#[test]
fn a_possibly_string_key_is_never_guarded() {
    let body = vec![
        let_(
            10,
            false,
            Type::Named("Int32Array".into()),
            int32_new(Expr::Integer(8)),
        ),
        Stmt::Return(Some(get(10, Expr::LocalGet(1)))),
    ];
    let ir = probe_ir("gview_any_key", vec![param(1, Type::Any)], body);
    assert!(!ir.contains("pview.gget"), "{ir}");
}

/// `new Int32Array(m)` with `m` an integer local owns its storage, so the
/// guarded tier serves it. With a parameter, only a clone whose entry proved
/// the argument a Number may; the public body sees any caller's argument, an
/// ArrayBuffer included, and must not.
#[test]
fn a_non_object_length_owns_the_storage_and_a_parameter_does_not() {
    let counted = vec![
        let_(20, true, Type::Number, Expr::Integer(4)),
        Stmt::Expr(Expr::LocalSet(
            20,
            Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::LocalGet(20)),
                right: Box::new(Expr::Integer(4)),
            }),
        )),
    ];
    let owned = probe_ir(
        "gview_int_len",
        vec![],
        swap_body(int32_new(Expr::LocalGet(20)), counted),
    );
    assert!(owned.contains("pview.gget.load"), "{owned}");

    let public = probe_functions(
        "gview_param_len",
        vec![param(1, Type::Number)],
        swap_body(int32_new(Expr::LocalGet(1)), vec![]),
    )
    .into_iter()
    .find(|(symbol, _)| !symbol.contains('$'))
    .expect("public probe entry")
    .1;
    assert!(!public.contains("pview."), "{public}");
}
