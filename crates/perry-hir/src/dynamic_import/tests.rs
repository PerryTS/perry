use super::*;
use crate::ir::Module;
use perry_types::Type;

#[test]
fn resolve_string_literal() {
    let r = resolve_import_path(&Expr::String("./foo.ts".into()));
    match r {
        Resolution::Set(v) => assert_eq!(v, vec!["./foo.ts"]),
        _ => panic!("expected Set"),
    }
}

#[test]
fn resolve_ternary_of_literals() {
    let r = resolve_import_path(&Expr::Conditional {
        condition: Box::new(Expr::Bool(true)),
        then_expr: Box::new(Expr::String("./a.ts".into())),
        else_expr: Box::new(Expr::String("./b.ts".into())),
    });
    match r {
        Resolution::Set(v) => {
            assert_eq!(v.len(), 2);
            assert!(v.contains(&"./a.ts".to_string()));
            assert!(v.contains(&"./b.ts".to_string()));
        }
        _ => panic!("expected Set"),
    }
}

#[test]
fn resolve_ternary_dedupes() {
    let r = resolve_import_path(&Expr::Conditional {
        condition: Box::new(Expr::Bool(true)),
        then_expr: Box::new(Expr::String("./a.ts".into())),
        else_expr: Box::new(Expr::String("./a.ts".into())),
    });
    match r {
        Resolution::Set(v) => assert_eq!(v, vec!["./a.ts"]),
        _ => panic!("expected Set"),
    }
}

#[test]
fn resolve_unresolvable_local() {
    let r = resolve_import_path(&Expr::LocalGet(0));
    assert!(matches!(r, Resolution::Unresolved(_)));
}

#[test]
fn tla_detects_module_init_await() {
    let mut m = Module::new("t");
    m.init
        .push(Stmt::Expr(Expr::Await(Box::new(Expr::Undefined))));
    detect_top_level_await(&mut m);
    assert!(m.has_top_level_await);
}

#[test]
fn resolve_template_literal_with_const_local() {
    // Simulate the HIR shape produced by `lower_tpl` for
    // `./locale_${lang}.ts` where lang is a module-level const.
    // The Add chain is `("./locale_" + lang) + ".ts"`.
    let arg = Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::String("./locale_".into())),
            right: Box::new(Expr::LocalGet(7)),
        }),
        right: Box::new(Expr::String(".ts".into())),
    };
    let mut consts = std::collections::HashMap::new();
    consts.insert(7u32, Expr::String("es".into()));
    let mut visiting = std::collections::HashSet::new();
    let r = resolve_import_path_with_consts(&arg, &consts, &mut visiting);
    match r {
        Resolution::Set(v) => assert_eq!(v, vec!["./locale_es.ts"]),
        _ => panic!("expected Set"),
    }
}

#[test]
fn resolve_template_literal_with_ternary_interpolation() {
    // `./locale_${cond ? 'en' : 'es'}.ts` — Cartesian product.
    let interp = Expr::Conditional {
        condition: Box::new(Expr::Bool(true)),
        then_expr: Box::new(Expr::String("en".into())),
        else_expr: Box::new(Expr::String("es".into())),
    };
    let arg = Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::String("./locale_".into())),
            right: Box::new(interp),
        }),
        right: Box::new(Expr::String(".ts".into())),
    };
    let consts: std::collections::HashMap<u32, Expr> = std::collections::HashMap::new();
    let mut visiting = std::collections::HashSet::new();
    let r = resolve_import_path_with_consts(&arg, &consts, &mut visiting);
    match r {
        Resolution::Set(v) => {
            assert_eq!(v.len(), 2);
            assert!(v.contains(&"./locale_en.ts".to_string()));
            assert!(v.contains(&"./locale_es.ts".to_string()));
        }
        _ => panic!("expected Set"),
    }
}

#[test]
fn resolve_local_const_propagation() {
    // `const p = './foo.ts'; import(p)`
    let arg = Expr::LocalGet(3);
    let mut consts = std::collections::HashMap::new();
    consts.insert(3u32, Expr::String("./foo.ts".into()));
    let mut visiting = std::collections::HashSet::new();
    let r = resolve_import_path_with_consts(&arg, &consts, &mut visiting);
    match r {
        Resolution::Set(v) => assert_eq!(v, vec!["./foo.ts"]),
        _ => panic!("expected Set"),
    }
}

#[test]
fn resolve_unresolved_param_local() {
    // `function f(p) { import(p) }` — p isn't in the const map.
    let arg = Expr::LocalGet(42);
    let consts: std::collections::HashMap<u32, Expr> = std::collections::HashMap::new();
    let mut visiting = std::collections::HashSet::new();
    let r = resolve_import_path_with_consts(&arg, &consts, &mut visiting);
    assert!(matches!(r, Resolution::Unresolved(_)));
}

#[test]
fn resolve_param_string_literal_union() {
    let arg = Expr::LocalGet(42);
    let consts: std::collections::HashMap<u32, Expr> = std::collections::HashMap::new();
    let mut params = std::collections::HashMap::new();
    params.insert(42, vec!["./a.ts".to_string(), "./b.ts".to_string()]);
    let mut visiting = std::collections::HashSet::new();
    match resolve_import_path_with_consts_and_params(&arg, &consts, &params, &mut visiting) {
        Resolution::Set(v) => assert_eq!(v, vec!["./a.ts", "./b.ts"]),
        Resolution::Unresolved(reason) => panic!("expected Set, got Unresolved: {reason}"),
    }
}

#[test]
fn collect_param_string_literal_union_from_function() {
    let mut m = Module::new("t");
    m.functions.push(Function {
        id: 1,
        name: "load".to_string(),
        type_params: Vec::new(),
        params: vec![
            Param {
                id: 42,
                name: "specifier".to_string(),
                ty: Type::Union(vec![
                    Type::StringLiteral("./a.ts".to_string()),
                    Type::StringLiteral("./b.ts".to_string()),
                ]),
                default: None,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            },
            Param {
                id: 43,
                name: "broad".to_string(),
                ty: Type::Union(vec![
                    Type::StringLiteral("./c.ts".to_string()),
                    Type::String,
                ]),
                default: None,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            },
        ],
        return_type: Type::Any,
        body: Vec::new(),
        is_async: true,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });

    let params = collect_dynamic_import_param_literals(&m);
    assert_eq!(
        params.get(&42),
        Some(&vec!["./a.ts".to_string(), "./b.ts".to_string()])
    );
    assert!(
        !params.contains_key(&43),
        "mixed literal/broad string unions are not finite"
    );
}

#[test]
fn collect_param_string_literal_union_from_type_alias() {
    let mut m = Module::new("t");
    m.functions.push(Function {
        id: 1,
        name: "load".to_string(),
        type_params: Vec::new(),
        params: vec![
            Param {
                id: 42,
                name: "specifier".to_string(),
                ty: Type::Named("Specifier".to_string()),
                default: None,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            },
            Param {
                id: 43,
                name: "chained".to_string(),
                ty: Type::Named("ChainedSpecifier".to_string()),
                default: None,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            },
            Param {
                id: 44,
                name: "mixed".to_string(),
                ty: Type::Named("MixedSpecifier".to_string()),
                default: None,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            },
            Param {
                id: 45,
                name: "cycle".to_string(),
                ty: Type::Named("CycleA".to_string()),
                default: None,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            },
        ],
        return_type: Type::Any,
        body: Vec::new(),
        is_async: true,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    m.type_aliases.push(crate::ir::TypeAlias {
        id: 1,
        name: "Specifier".to_string(),
        type_params: Vec::new(),
        ty: Type::Union(vec![
            Type::StringLiteral("./a.ts".to_string()),
            Type::StringLiteral("./b.ts".to_string()),
        ]),
        is_exported: false,
    });
    m.type_aliases.push(crate::ir::TypeAlias {
        id: 2,
        name: "ChainedSpecifier".to_string(),
        type_params: Vec::new(),
        ty: Type::Named("Specifier".to_string()),
        is_exported: false,
    });
    m.type_aliases.push(crate::ir::TypeAlias {
        id: 3,
        name: "MixedSpecifier".to_string(),
        type_params: Vec::new(),
        ty: Type::Union(vec![
            Type::StringLiteral("./c.ts".to_string()),
            Type::String,
        ]),
        is_exported: false,
    });
    m.type_aliases.push(crate::ir::TypeAlias {
        id: 4,
        name: "CycleA".to_string(),
        type_params: Vec::new(),
        ty: Type::Named("CycleB".to_string()),
        is_exported: false,
    });
    m.type_aliases.push(crate::ir::TypeAlias {
        id: 5,
        name: "CycleB".to_string(),
        type_params: Vec::new(),
        ty: Type::Named("CycleA".to_string()),
        is_exported: false,
    });

    let params = collect_dynamic_import_param_literals(&m);
    let expected = vec!["./a.ts".to_string(), "./b.ts".to_string()];
    assert_eq!(params.get(&42), Some(&expected));
    assert_eq!(params.get(&43), Some(&expected));
    assert!(
        !params.contains_key(&44),
        "mixed literal/broad string aliases are not finite"
    );
    assert!(!params.contains_key(&45), "cyclic aliases are not finite");
}

#[test]
fn collect_consts_skips_mutated() {
    let mut m = Module::new("t");
    m.init.push(Stmt::Let {
        id: 1,
        name: "stable".into(),
        ty: perry_types::Type::String,
        mutable: false,
        init: Some(Expr::String("./a.ts".into())),
    });
    m.init.push(Stmt::Let {
        id: 2,
        name: "mutated".into(),
        ty: perry_types::Type::String,
        mutable: false,
        init: Some(Expr::String("./b.ts".into())),
    });
    m.init.push(Stmt::Expr(Expr::LocalSet(
        2,
        Box::new(Expr::String("./c.ts".into())),
    )));
    let consts = collect_module_const_locals(&m);
    assert!(consts.contains_key(&1));
    assert!(!consts.contains_key(&2));
}

#[test]
fn collect_includes_unreassigned_let_but_drops_reassigned() {
    // #1674: a `let` (mutable) that is never reassigned resolves like a
    // const; a reassigned one still falls back to Unresolved.
    let mut m = Module::new("t");
    m.init.push(Stmt::Let {
        id: 1,
        name: "stableLet".into(),
        ty: perry_types::Type::String,
        mutable: true,
        init: Some(Expr::String("./a.ts".into())),
    });
    m.init.push(Stmt::Let {
        id: 2,
        name: "reassignedLet".into(),
        ty: perry_types::Type::String,
        mutable: true,
        init: Some(Expr::String("./b.ts".into())),
    });
    m.init.push(Stmt::Expr(Expr::LocalSet(
        2,
        Box::new(Expr::String("./c.ts".into())),
    )));
    let consts = collect_module_const_locals(&m);
    assert!(matches!(consts.get(&1).map(Borrow::borrow), Some(Expr::String(s)) if s == "./a.ts"));
    assert!(!consts.contains_key(&2));
}

#[test]
fn resolve_unreassigned_let_ternary_union() {
    // The #1674 acceptance shape: `let p = cond ? './a.ts' : './b.ts'`.
    let mut m = Module::new("t");
    m.init.push(Stmt::Let {
        id: 5,
        name: "p".into(),
        ty: perry_types::Type::String,
        mutable: true,
        init: Some(Expr::Conditional {
            condition: Box::new(Expr::Bool(true)),
            then_expr: Box::new(Expr::String("./a.ts".into())),
            else_expr: Box::new(Expr::String("./b.ts".into())),
        }),
    });
    let consts = collect_module_const_locals(&m);
    let mut visiting = std::collections::HashSet::new();
    match resolve_import_path_with_consts(&Expr::LocalGet(5), &consts, &mut visiting) {
        Resolution::Set(mut v) => {
            v.sort();
            assert_eq!(v, vec!["./a.ts", "./b.ts"]);
        }
        Resolution::Unresolved(reason) => panic!("expected Set, got Unresolved: {reason}"),
    }
}

#[test]
fn resolve_reassigned_local_literal_candidates() {
    let mut m = Module::new("t");
    m.init.push(Stmt::Let {
        id: 5,
        name: "p".into(),
        ty: Type::String,
        mutable: true,
        init: None,
    });
    m.init.push(Stmt::If {
        condition: Expr::Bool(true),
        then_branch: vec![Stmt::Expr(Expr::LocalSet(
            5,
            Box::new(Expr::String("./a.ts".into())),
        ))],
        else_branch: Some(vec![Stmt::Expr(Expr::LocalSet(
            5,
            Box::new(Expr::String("./b.ts".into())),
        ))]),
    });

    let consts = collect_module_const_locals(&m);
    let params = collect_dynamic_import_param_literals(&m);
    let locals = collect_dynamic_import_local_candidate_literals(&m, &consts, &params);
    assert_eq!(
        locals.get(&5),
        Some(&vec!["./a.ts".to_string(), "./b.ts".to_string()])
    );

    let mut visiting = HashSet::new();
    match resolve_import_path_with_context(
        &Expr::LocalGet(5),
        &consts,
        &params,
        &locals,
        &mut visiting,
    ) {
        Resolution::Set(v) => assert_eq!(v, vec!["./a.ts", "./b.ts"]),
        Resolution::Unresolved(reason) => panic!("expected Set, got Unresolved: {reason}"),
    }
}

#[test]
fn reassigned_local_candidates_drop_mixed_dynamic_defs() {
    let mut m = Module::new("t");
    m.init.push(Stmt::Let {
        id: 5,
        name: "p".into(),
        ty: Type::String,
        mutable: true,
        init: None,
    });
    m.init.push(Stmt::Expr(Expr::LocalSet(
        5,
        Box::new(Expr::String("./a.ts".into())),
    )));
    m.init
        .push(Stmt::Expr(Expr::LocalSet(5, Box::new(Expr::LocalGet(99)))));

    let consts = collect_module_const_locals(&m);
    let params = collect_dynamic_import_param_literals(&m);
    let locals = collect_dynamic_import_local_candidate_literals(&m, &consts, &params);
    assert!(
        !locals.contains_key(&5),
        "any non-resolvable assignment keeps the import site unresolved"
    );
}

#[test]
fn resolve_closure_local_const_specifier() {
    // #1725: `() => { const cfWorkers = "cloudflare:workers"; import(cfWorkers) }`
    // — the const lives inside a closure body (hono's getColorEnabledAsync
    // IIFE shape), not at module top level. It must be collected so the
    // specifier resolves instead of erroring "not a module-level const".
    let mut m = Module::new("t");
    let closure = Expr::Closure {
        func_id: 0,
        params: vec![],
        return_type: Type::Any,
        body: vec![Stmt::Let {
            id: 9,
            name: "cfWorkers".into(),
            ty: Type::String,
            mutable: false,
            init: Some(Expr::String("cloudflare:workers".into())),
        }],
        captures: vec![],
        mutable_captures: vec![],
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: false,
        is_async: true,
        is_generator: false,
        is_strict: false,
    };
    m.init.push(Stmt::Expr(closure));

    let consts = collect_module_const_locals(&m);
    assert!(
        consts.contains_key(&9),
        "const declared inside a closure body should be collected"
    );

    let mut visiting = std::collections::HashSet::new();
    match resolve_import_path_with_consts(&Expr::LocalGet(9), &consts, &mut visiting) {
        Resolution::Set(v) => assert_eq!(v, vec!["cloudflare:workers"]),
        other => panic!("expected resolved Set, got {:?}", other),
    }
}

#[test]
fn collect_consts_invalidates_closure_mutation() {
    // Soundness: a binding reassigned inside a closure body must be dropped
    // from the const map (the mutation scan descends into closures, #1725).
    let mut m = Module::new("t");
    m.init.push(Stmt::Let {
        id: 5,
        name: "p".into(),
        ty: Type::String,
        mutable: false,
        init: Some(Expr::String("./a.ts".into())),
    });
    let closure = Expr::Closure {
        func_id: 0,
        params: vec![],
        return_type: Type::Any,
        body: vec![Stmt::Expr(Expr::LocalSet(
            5,
            Box::new(Expr::String("./b.ts".into())),
        ))],
        captures: vec![5],
        mutable_captures: vec![5],
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: false,
        is_async: false,
        is_generator: false,
        is_strict: false,
    };
    m.init.push(Stmt::Expr(closure));
    let consts = collect_module_const_locals(&m);
    assert!(
        !consts.contains_key(&5),
        "mutation inside closure must invalidate"
    );
}

#[test]
fn flatten_local_named_exports() {
    let mut m = Module::new("foo");
    m.exports.push(Export::Named {
        local: "x".into(),
        exported: "x".into(),
    });
    m.exports.push(Export::Named {
        local: "_g".into(),
        exported: "greet".into(),
    });
    let map = std::collections::HashMap::from([("foo".to_string(), m.clone())]);
    let lookup = |s: &str| map.get(s);
    let flat = flatten_exports("foo", &lookup);
    assert_eq!(flat.len(), 2);
    assert_eq!(flat[0].name, "x");
    assert_eq!(flat[0].source_module, "foo");
    assert_eq!(flat[0].source_local, "x");
    assert_eq!(flat[1].name, "greet");
    assert_eq!(flat[1].source_local, "_g");
}

#[test]
fn flatten_reexport_one_hop() {
    let mut barrel = Module::new("barrel");
    barrel.exports.push(Export::ReExport {
        source: "inner".into(),
        imported: "v".into(),
        exported: "v".into(),
    });
    let map = std::collections::HashMap::from([("barrel".to_string(), barrel.clone())]);
    let lookup = |s: &str| map.get(s);
    let flat = flatten_exports("barrel", &lookup);
    assert_eq!(flat.len(), 1);
    assert_eq!(flat[0].name, "v");
    assert_eq!(flat[0].source_module, "inner");
    assert_eq!(flat[0].source_local, "v");
}

#[test]
fn flatten_export_all_recursive() {
    let mut inner = Module::new("inner");
    inner.exports.push(Export::Named {
        local: "v".into(),
        exported: "v".into(),
    });
    let mut barrel = Module::new("barrel");
    barrel.exports.push(Export::ExportAll {
        source: "inner".into(),
    });
    let map = std::collections::HashMap::from([
        ("inner".to_string(), inner.clone()),
        ("barrel".to_string(), barrel.clone()),
    ]);
    let lookup = |s: &str| map.get(s);
    let flat = flatten_exports("barrel", &lookup);
    assert_eq!(flat.len(), 1);
    assert_eq!(flat[0].name, "v");
    assert_eq!(flat[0].source_module, "inner");
    assert_eq!(flat[0].source_local, "v");
}

#[test]
fn flatten_export_all_cycle_safe() {
    // a -> b -> a — must terminate.
    let mut a = Module::new("a");
    a.exports.push(Export::ExportAll { source: "b".into() });
    a.exports.push(Export::Named {
        local: "fromA".into(),
        exported: "fromA".into(),
    });
    let mut b = Module::new("b");
    b.exports.push(Export::ExportAll { source: "a".into() });
    b.exports.push(Export::Named {
        local: "fromB".into(),
        exported: "fromB".into(),
    });
    let map = std::collections::HashMap::from([
        ("a".to_string(), a.clone()),
        ("b".to_string(), b.clone()),
    ]);
    let lookup = |s: &str| map.get(s);
    let flat = flatten_exports("a", &lookup);
    // Both names appear; recursion terminates at the back-edge.
    let names: Vec<String> = flat.iter().map(|e| e.name.clone()).collect();
    assert!(names.contains(&"fromA".to_string()));
    assert!(names.contains(&"fromB".to_string()));
}

#[test]
fn flatten_namespace_re_export() {
    let mut m = Module::new("m");
    m.exports.push(Export::NamespaceReExport {
        source: "sub".into(),
        name: "Sub".into(),
    });
    let map = std::collections::HashMap::from([("m".to_string(), m.clone())]);
    let lookup = |s: &str| map.get(s);
    let flat = flatten_exports("m", &lookup);
    assert_eq!(flat.len(), 1);
    assert_eq!(flat[0].name, "Sub");
    assert_eq!(flat[0].nested_namespace_of, Some("sub".to_string()));
}

#[test]
fn tla_skips_await_inside_closure() {
    let mut m = Module::new("t");
    // Build a closure body containing an Await — the module-level
    // detector must NOT descend into the closure.
    let closure = Expr::Closure {
        func_id: 0,
        params: vec![],
        return_type: Type::Any,
        body: vec![Stmt::Expr(Expr::Await(Box::new(Expr::Undefined)))],
        captures: vec![],
        mutable_captures: vec![],
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: false,
        is_async: true,
        is_generator: false,
        is_strict: false,
    };
    m.init.push(Stmt::Expr(closure));
    detect_top_level_await(&mut m);
    assert!(!m.has_top_level_await);
}

// #1674 sub-B: `("./plugins/" + name) + ".ts"` where `name` is a
// non-resolvable local — the HIR shape of `` `./plugins/${name}.ts` ``.
fn glob_chain(prefix: &str, suffix: &str, wild_id: u32) -> Expr {
    Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::String(prefix.into())),
            right: Box::new(Expr::LocalGet(wild_id)),
        }),
        right: Box::new(Expr::String(suffix.into())),
    }
}

#[test]
fn glob_pattern_extracts_relative_prefix_and_suffix() {
    let consts: std::collections::HashMap<u32, Expr> = std::collections::HashMap::new();
    let arg = glob_chain("./plugins/", ".ts", 1);
    assert_eq!(
        dynamic_import_glob_pattern(&arg, &consts),
        Some(("./plugins/".to_string(), ".ts".to_string()))
    );
}

#[test]
fn glob_pattern_rejects_non_relative_or_dirless_prefix() {
    let consts: std::collections::HashMap<u32, Expr> = std::collections::HashMap::new();
    // bare prefix with no directory component — too broad to glob.
    assert_eq!(
        dynamic_import_glob_pattern(&glob_chain("locale_", ".ts", 1), &consts),
        None
    );
    // absolute / package prefix — not a relative directory glob.
    assert_eq!(
        dynamic_import_glob_pattern(&glob_chain("@scope/", ".ts", 1), &consts),
        None
    );
}

#[test]
fn glob_pattern_none_when_fully_resolvable() {
    // No wildcard part — the normal resolver handles this, not the glob.
    let consts: std::collections::HashMap<u32, Expr> = std::collections::HashMap::new();
    let arg = Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(Expr::String("./a".into())),
        right: Box::new(Expr::String(".ts".into())),
    };
    assert_eq!(dynamic_import_glob_pattern(&arg, &consts), None);
}
