//! S2 of the deferred-collection RFC (`expr/ic_fast_split.rs`): each split
//! site's fast call is a GC-leaf call RS4GC leaves alone, and its slow call is
//! the ONLY statepoint, still carrying the caller's live GC values. The
//! static-key store site is no longer split: its probe and `[[Set]]` are ONE
//! out-of-line call (`perry_store_or_put`), the site's only statepoint.
//!
//! Asserted on the post-RS4GC IR of the production pass string, on both
//! native-roots targets. Both halves are non-vacuous:
//!
//! * the collecting-call claims go through [`Statepoints::at`], which panics
//!   when the callee produced no safepoint, and require a NON-EMPTY live set
//!   (the fixture keeps a fresh object live across every access);
//! * the leaf-call claims ("no statepoint at `_fast`") are paired with a
//!   differential control: the same IR with the attribute stripped from the
//!   fast call DOES produce a statepoint there, so the absence is the
//!   attribute's doing and not a parse miss.

use super::*;
use perry_hir::{Class, ClassField};

/// The store site's one out-of-line call, and the calls it replaced.
const STORE_SITE_CALL: &str = "perry_store_or_put";
const STORE_SITE_GONE: [&str; 2] = ["perry_probe_set", "js_put_value_set"];

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
                byte_offset: 0,
                byte_end: 0,
            }),
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::LocalGet(103)),
                property: "x".to_string(),
                value: Box::new(Expr::Number(7.0)),
                byte_offset: 0,
                byte_end: 0,
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
                    byte_end: 0,
                    callee: Box::new(Expr::LocalGet(102)),
                    args: vec![Expr::LocalGet(22)],
                    type_args: Vec::new(),
                    byte_offset: 0,
                },
            ),
            Stmt::Return(Some(Expr::Array(
                vec![
                    Expr::LocalGet(20),
                    Expr::LocalGet(21),
                    Expr::LocalGet(22),
                    Expr::LocalGet(23),
                    Expr::LocalGet(24),
                ],
                None,
            ))),
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

/// Remove `"gc-leaf-function"` from every call to `callee` — the control arm.
fn strip_leaf(ir: &str, callee: &str) -> String {
    let marker = format!("@{callee}(");
    ir.lines()
        .map(|line| {
            if line.contains(&marker) && line.contains("call ") {
                line.replace(" \"gc-leaf-function\"", "")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_store_site_is_one_collecting_call() {
    let module = split_module();
    assert!(
        crate::codegen::decide_full_outline_ic(crate::codegen::module_callable_count(&module)),
        "test premise: the fixture must cross the full-outline threshold"
    );
    for target in NATIVE_TARGETS {
        let ir = native_ir(&module, target, false);
        let probe = probe_body_symbol(&ir, "ic_fast_split.ts");
        let body = function_slice(&ir, &probe);
        let points = statepoints_of(&ir, target, &probe);

        let calls: Vec<&str> = body
            .lines()
            .filter(|l| l.contains(&format!("@{STORE_SITE_CALL}(")) && l.contains("call "))
            .collect();
        assert!(
            !calls.is_empty(),
            "[{target}] test premise: `{STORE_SITE_CALL}` is emitted in @{probe}"
        );
        for gone in STORE_SITE_GONE {
            assert!(
                !body.contains(&format!("@{gone}(")),
                "[{target}] the store site must not call `{gone}` inline any more"
            );
        }
        let at = points.at(STORE_SITE_CALL);
        assert_eq!(
            at.len(),
            calls.len(),
            "[{target}] every `{STORE_SITE_CALL}` call is a statepoint"
        );
        for sp in at {
            assert!(
                !sp.live.is_empty(),
                "[{target}] the store call must still relocate the live object: {sp:?}"
            );
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

#[test]
fn read_site_memos_preserve_leaf_and_collecting_root_contracts() {
    for outlined in [false, true] {
        let mut module = split_module();
        if !outlined {
            module.functions.retain(|f| f.id == 1);
        }
        let function = module.functions.iter_mut().find(|f| f.id == 1).unwrap();
        function.params.push(param(104, "key", Type::String));
        function.body.insert(
            2,
            let_stmt(
                25,
                "keyed",
                Expr::IndexGet {
                    element_offset: 0,
                    byte_offset: 0,
                    byte_end: 0,
                    object: Box::new(Expr::LocalGet(100)),
                    index: Box::new(Expr::LocalGet(104)),
                },
            ),
        );
        let Some(Stmt::Return(Some(Expr::Array(values, _)))) = function.body.last_mut() else {
            panic!("fixture must retain the lookup result");
        };
        values.push(Expr::LocalGet(25));
        assert_eq!(
            crate::codegen::decide_full_outline_ic(crate::codegen::module_callable_count(&module)),
            outlined,
        );
        for target in NATIVE_TARGETS {
            let ir = native_ir(&module, target, false);
            let probe = probe_body_symbol(&ir, "ic_fast_split.ts");
            let body = function_slice(&ir, &probe);
            let points = statepoints_of(&ir, target, &probe);
            // A fixed-name leaf owns no site storage: the receiver's shape
            // answers (accessors included) and a decline takes the memo-free
            // collecting exit. The keyed site owns none either.
            let fixed = if outlined {
                ("perry_probe_get_value", "js_object_get_field_value")
            } else {
                ("perry_probe_get", "js_object_get_field_value")
            };
            for (fast, slow) in [
                fixed,
                ("perry_probe_get_keyed", "js_object_get_keyed_slow"),
            ] {
                let calls: Vec<_> = body
                    .lines()
                    .filter(|line| line.contains("call ") && line.contains(&format!("@{fast}(")))
                    .collect();
                assert!(
                    !calls.is_empty(),
                    "[{target}] missing {fast} in outlined={outlined}: {body}"
                );
                assert!(calls
                    .iter()
                    .all(|line| line.contains("\"gc-leaf-function\"")));
                assert!(points.iter().all(|sp| sp.callee != fast));
                let collecting = points.at(slow);
                assert!(
                    !collecting.is_empty(),
                    "[{target}] missing collecting {slow}"
                );
                for sp in collecting {
                    assert!(
                        !sp.live.is_empty(),
                        "[{target}] {slow} must retain live roots: {sp:?}"
                    );
                }
                let control = statepoints_of(&strip_leaf(&ir, fast), target, &probe);
                assert!(!control.at(fast).is_empty());
            }
        }
    }
}
