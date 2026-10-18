//! Conservative store-probe rooting and the remaining inline fast/slow splits.
//! Runtime call effects now come from the linked archive sidecars. This unit
//! fixture proves no Leaf fact for the store probe, so it must retain roots.

use super::*;
use perry_hir::{Class, ClassField};

const FAST_SLOW: [(&str, &str); 1] = [("perry_probe_set", "js_put_value_set")];

fn point_class() -> Class {
    Class {
        id: 101,
        name: "Point".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: vec![ClassField {
            origin: perry_hir::ClassFieldOrigin::Definition,
            name: "x".to_string(),
            key_expr: None,
            ty: Type::Number,
            init: None,
            is_private: false,
            is_readonly: false,
            decorators: Vec::new(),
        }],
        constructor: None,
        methods: Vec::new(),
        getters: Vec::new(),
        setters: Vec::new(),
        static_accessor_names: Vec::new(),
        static_accessor_fn_ids: Vec::new(),
        computed_members: Vec::new(),
        static_fields: Vec::new(),
        static_methods: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        aliases: Vec::new(),
        is_nested: false,
        alloc_width_hint: 0,
        specialized_from: None,
    }
}

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

/// `probe(o: any, p: Point, f: any, q: Point)`: a fresh object stays live across a
/// generic read, a class-field read, a class-field write, a template coercion
/// and a dynamic call. 4,000 empty padding functions push the module past the
/// full-outline threshold, which is where the IC splits apply.
fn split_module() -> Module {
    let mut module = bare_module("ic_fast_split.ts");
    module.classes = vec![point_class()];
    module.functions = (0..4000u32)
        .map(|i| Function {
            id: 10_000 + i,
            name: format!("pad{i}"),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Type::Void,
            body: Vec::new(),
            is_async: false,
            is_generator: false,
            is_strict: true,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        })
        .collect();
    module.functions.push(Function {
        id: 1,
        name: "probe".to_string(),
        type_params: Vec::new(),
        params: vec![
            param(100, "o", Type::Any),
            param(101, "p", Type::Named("Point".to_string())),
            param(102, "f", Type::Any),
            param(103, "q", Type::Named("Point".to_string())),
        ],
        return_type: Type::Any,
        body: vec![
            let_stmt(20, "keep", heap_value()),
            let_stmt(21, "a", field_get(100, "foo")),
            let_stmt(22, "b", field_get(101, "x")),
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::LocalGet(100)),
                property: "bar".to_string(),
                value: Box::new(Expr::LocalGet(22)),
            }),
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::LocalGet(103)),
                property: "x".to_string(),
                value: Box::new(Expr::Number(7.0)),
            }),
            let_stmt(
                23,
                "s",
                Expr::TemplateStringCoerce(Box::new(Expr::LocalGet(21))),
            ),
            let_stmt(
                24,
                "r",
                Expr::Call {
                    callee: Box::new(Expr::LocalGet(102)),
                    args: vec![Expr::LocalGet(22)],
                    type_args: Vec::new(),
                    byte_offset: 0,
                },
            ),
            Stmt::Return(Some(Expr::Array(vec![
                Expr::LocalGet(20),
                Expr::LocalGet(21),
                Expr::LocalGet(22),
                Expr::LocalGet(23),
                Expr::LocalGet(24),
            ]))),
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

/// Without an archive sidecar proving the store probe Leaf, both calls must
/// remain conservative safepoints. Removing a statepoint would lose live roots.
#[test]
fn store_probe_and_complete_set_keep_conservative_statepoints() {
    let module = split_module();
    assert!(crate::codegen::decide_full_outline_ic(
        crate::codegen::module_callable_count(&module)
    ));
    for target in NATIVE_TARGETS {
        let ir = native_ir(&module, target, false);
        let probe = probe_body_symbol(&ir, "ic_fast_split.ts");
        let body = function_slice(&ir, &probe);
        let points = statepoints_of(&ir, target, &probe);
        for (probe, slow) in FAST_SLOW {
            for callee in [probe, slow] {
                assert!(body
                    .lines()
                    .any(|line| line.contains(&format!("@{callee}(")) && line.contains("call ")));
                for point in points.at(callee) {
                    assert!(
                        !point.live.is_empty(),
                        "[{target}] {callee} must retain the live object: {point:?}"
                    );
                }
            }
        }
    }
}

/// The two inline splits: the hit is no call at all, and the original helper
/// is called only from the cold block.
#[test]
fn inline_splits_call_their_helper_only_from_the_cold_block() {
    let module = split_module();
    for target in NATIVE_TARGETS {
        let ir = native_ir(&module, target, false);
        let probe = probe_body_symbol(&ir, "ic_fast_split.ts");
        let body = function_slice(&ir, &probe);
        for (helper, cold_prefix) in [
            ("js_template_string_coerce_box", "tmpl_coerce.slow"),
            ("js_closure_unbox_callee_checked", "callee_unbox.slow"),
        ] {
            let mut current = String::new();
            let mut calls = 0;
            for line in body.lines() {
                if !line.starts_with(char::is_whitespace) && line.ends_with(':') {
                    current = line.trim_end_matches(':').to_string();
                } else if line.contains(&format!("@{helper}(")) {
                    calls += 1;
                    assert!(
                        current.starts_with(cold_prefix),
                        "[{target}] `{helper}` called outside `{cold_prefix}`: block {current}"
                    );
                }
            }
            assert_eq!(
                calls, 1,
                "[{target}] `{helper}` must be called exactly once"
            );
            let points = statepoints_of(&ir, target, &probe);
            assert!(!points.at(helper).is_empty());
        }
    }
}
