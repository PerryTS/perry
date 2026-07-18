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

// #5207: registry-object dynamic-import patterns. A const object literal maps
// route/feature keys to relative chunk paths; member access resolves to the
// union of the registry's (relative) value specifiers — the whole chunk set is
// what we ingest, and the runtime dispatch picks the right one by path string.
//   `const R = { a: "./chunk-a.js", b: "./chunk-b.js" };`
fn open_chunk_registry() -> Expr {
    Expr::Object(vec![
        ("a".to_string(), Expr::String("./chunk-a.js".into())),
        ("b".to_string(), Expr::String("./chunk-b.js".into())),
    ])
}

// The same literal as Perry actually lowers a closed-shape object: a
// `new __AnonShape_…(value0, value1)` whose args are the field values in order.
fn closed_chunk_registry() -> Expr {
    Expr::New {
        class_name: "__AnonShape_deadbeef".to_string(),
        args: vec![
            Expr::String("./chunk-a.js".into()),
            Expr::String("./chunk-b.js".into()),
        ],
        type_args: Vec::new(),
        byte_offset: 0,
        cap_args_appended: 0,
    }
}

fn assert_resolves_both_chunks(arg: &Expr) {
    let mut consts = std::collections::HashMap::new();
    consts.insert(5u32, open_chunk_registry());
    let mut visiting = std::collections::HashSet::new();
    match resolve_import_path_with_consts(arg, &consts, &mut visiting) {
        Resolution::Set(v) => {
            assert_eq!(v.len(), 2);
            assert!(v.contains(&"./chunk-a.js".to_string()));
            assert!(v.contains(&"./chunk-b.js".to_string()));
        }
        Resolution::Unresolved(reason) => panic!("expected Set, got Unresolved: {reason}"),
    }
}

#[test]
fn resolve_registry_computed_key_enumerates_all() {
    // `import(R[key])` with a runtime key — the core registry pattern.
    assert_resolves_both_chunks(&Expr::IndexGet {
        object: Box::new(Expr::LocalGet(5)),
        index: Box::new(Expr::LocalGet(99)), // not a known literal
    });
}

#[test]
fn resolve_registry_static_property_enumerates_all() {
    // `import(R.a)` — over-approximate to the whole (relative) chunk set.
    assert_resolves_both_chunks(&Expr::PropertyGet {
        object: Box::new(Expr::LocalGet(5)),
        property: "a".to_string(),
    });
}

#[test]
fn resolve_closed_shape_registry_enumerates_all() {
    // The realistic lowering: a `new __AnonShape_…(…)` closed-shape literal.
    let arg = Expr::IndexGet {
        object: Box::new(Expr::LocalGet(5)),
        index: Box::new(Expr::LocalGet(99)),
    };
    let mut consts = std::collections::HashMap::new();
    consts.insert(5u32, closed_chunk_registry());
    let mut visiting = std::collections::HashSet::new();
    match resolve_import_path_with_consts(&arg, &consts, &mut visiting) {
        Resolution::Set(v) => {
            assert_eq!(v.len(), 2);
            assert!(v.contains(&"./chunk-a.js".to_string()));
            assert!(v.contains(&"./chunk-b.js".to_string()));
        }
        Resolution::Unresolved(reason) => panic!("expected Set, got Unresolved: {reason}"),
    }
}

#[test]
fn resolve_non_relative_registry_defers() {
    // A plain data object (non-module values) indexed for a non-import reason
    // must stay deferred — never try to compile `"app"` / `"3000"` as modules.
    let data = Expr::Object(vec![
        ("name".to_string(), Expr::String("app".into())),
        ("port".to_string(), Expr::String("3000".into())),
    ]);
    let arg = Expr::IndexGet {
        object: Box::new(Expr::LocalGet(5)),
        index: Box::new(Expr::LocalGet(99)),
    };
    let mut consts = std::collections::HashMap::new();
    consts.insert(5u32, data);
    let mut visiting = std::collections::HashSet::new();
    assert!(matches!(
        resolve_import_path_with_consts(&arg, &consts, &mut visiting),
        Resolution::Unresolved(_)
    ));
}

#[test]
fn resolve_circular_registry_defers_without_overflow() {
    // `const R5 = { a: R6[x] }; const R6 = { b: R5[y] };` — a circular registry
    // is valid TS and reaches the resolver via the const map. The cross-call
    // cycle guard must defer (Unresolved) instead of recursing forever.
    let r5 = Expr::Object(vec![(
        "a".to_string(),
        Expr::IndexGet {
            object: Box::new(Expr::LocalGet(6)),
            index: Box::new(Expr::LocalGet(99)),
        },
    )]);
    let r6 = Expr::Object(vec![(
        "b".to_string(),
        Expr::IndexGet {
            object: Box::new(Expr::LocalGet(5)),
            index: Box::new(Expr::LocalGet(99)),
        },
    )]);
    let arg = Expr::IndexGet {
        object: Box::new(Expr::LocalGet(5)),
        index: Box::new(Expr::LocalGet(99)),
    };
    let mut consts = std::collections::HashMap::new();
    consts.insert(5u32, r5);
    consts.insert(6u32, r6);
    let mut visiting = std::collections::HashSet::new();
    assert!(matches!(
        resolve_import_path_with_consts(&arg, &consts, &mut visiting),
        Resolution::Unresolved(_)
    ));
}

#[test]
fn resolve_chained_distinct_registries_resolves() {
    // `const A = { x: "./a.js" }; const B = { y: A.x }; import(B[k])` — distinct
    // (non-cyclic) registries must still resolve through the indirection.
    let a = Expr::Object(vec![("x".to_string(), Expr::String("./a.js".into()))]);
    let b = Expr::Object(vec![(
        "y".to_string(),
        Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(1)),
            property: "x".to_string(),
        },
    )]);
    let arg = Expr::IndexGet {
        object: Box::new(Expr::LocalGet(2)),
        index: Box::new(Expr::LocalGet(99)),
    };
    let mut consts = std::collections::HashMap::new();
    consts.insert(1u32, a);
    consts.insert(2u32, b);
    let mut visiting = std::collections::HashSet::new();
    match resolve_import_path_with_consts(&arg, &consts, &mut visiting) {
        Resolution::Set(v) => assert_eq!(v, vec!["./a.js"]),
        Resolution::Unresolved(reason) => panic!("expected Set, got Unresolved: {reason}"),
    }
}

#[test]
fn resolve_non_registry_member_access_defers() {
    // `import(cfg.path)` where cfg is opaque — must keep deferring, not panic.
    let arg = Expr::PropertyGet {
        object: Box::new(Expr::LocalGet(7)),
        property: "path".to_string(),
    };
    let consts: std::collections::HashMap<u32, Expr> = std::collections::HashMap::new();
    let mut visiting = std::collections::HashSet::new();
    assert!(matches!(
        resolve_import_path_with_consts(&arg, &consts, &mut visiting),
        Resolution::Unresolved(_)
    ));
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
fn resolve_path_join_over_replaced_dirname() {
    // node-pty's Windows conout worker uses:
    //   new Worker(path.join(__dirname.replace('node_modules.asar',
    //     'node_modules.asar.unpacked'), 'worker/conoutSocketWorker.js'))
    // `__dirname` is already lowered to a string by the parser; the path
    // resolver still needs to fold the replace + join chain to a single
    // deterministic worker module.
    let expr = Expr::PathJoin(
        Box::new(Expr::StringReplace {
            string: Box::new(Expr::String(
                "D:/tmp/probe/node_modules/node-pty/lib".to_string(),
            )),
            pattern: Box::new(Expr::String("node_modules.asar".to_string())),
            replacement: Box::new(Expr::String("node_modules.asar.unpacked".to_string())),
        }),
        Box::new(Expr::String("worker/conoutSocketWorker.js".to_string())),
    );

    let mut visiting = HashSet::new();
    match resolve_import_path_with_context(
        &expr,
        &HashMap::<u32, Expr>::new(),
        &HashMap::new(),
        &HashMap::new(),
        &mut visiting,
    ) {
        Resolution::Set(v) => assert_eq!(
            v,
            vec!["D:/tmp/probe/node_modules/node-pty/lib/worker/conoutSocketWorker.js"]
        ),
        Resolution::Unresolved(reason) => panic!("expected Set, got Unresolved: {reason}"),
    }
}

#[test]
fn resolve_cjs_path_default_join_over_replaced_dirname_local() {
    let mut module = Module::new("node-pty-windows-conout");
    module.init.push(Stmt::Let {
        id: 81,
        name: "scriptPath".into(),
        ty: Type::String,
        mutable: true,
        init: Some(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::String(
                    "D:\\tmp\\probe\\node_modules\\node-pty\\lib".to_string(),
                )),
                property: "replace".to_string(),
            }),
            args: vec![
                Expr::String("node_modules.asar".to_string()),
                Expr::String("node_modules.asar.unpacked".to_string()),
            ],
            type_args: vec![],
            byte_offset: 0,
        }),
    });

    let filename = Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::NativeModuleRef("path".to_string())),
                property: "default".to_string(),
            }),
            property: "join".to_string(),
        }),
        args: vec![
            Expr::LocalGet(81),
            Expr::String("worker/conoutSocketWorker.js".to_string()),
        ],
        type_args: vec![],
        byte_offset: 0,
    };

    let consts = collect_module_const_locals(&module);
    let params = collect_dynamic_import_param_literals(&module);
    let locals = collect_dynamic_import_local_candidate_literals(&module, &consts, &params);
    let mut visiting = HashSet::new();
    match resolve_import_path_with_context(&filename, &consts, &params, &locals, &mut visiting) {
        Resolution::Set(v) => assert_eq!(
            v,
            vec!["D:/tmp/probe/node_modules/node-pty/lib/worker/conoutSocketWorker.js"]
        ),
        Resolution::Unresolved(reason) => panic!("expected Set, got Unresolved: {reason}"),
    }
}

#[test]
fn resolve_path_join_normalizes_relative_segments() {
    let expr = Expr::PathJoin(
        Box::new(Expr::String("/project/src/lib".to_string())),
        Box::new(Expr::String("../worker.js".to_string())),
    );

    let mut visiting = HashSet::new();
    match resolve_import_path_with_context(
        &expr,
        &HashMap::<u32, Expr>::new(),
        &HashMap::new(),
        &HashMap::new(),
        &mut visiting,
    ) {
        Resolution::Set(v) => assert_eq!(v, vec!["/project/src/worker.js"]),
        Resolution::Unresolved(reason) => panic!("expected Set, got Unresolved: {reason}"),
    }
}

#[test]
fn worker_new_visitor_descends_into_closure_bodies() {
    let mut module = Module::new("worker-closure");
    module.init.push(Stmt::Expr(Expr::Closure {
        func_id: 1,
        params: vec![],
        return_type: Type::Void,
        body: vec![Stmt::Expr(Expr::WorkerNew {
            paths: vec![],
            filename: Box::new(Expr::String("./worker.js".to_string())),
            options: None,
            is_eval: false,
        })],
        captures: vec![],
        mutable_captures: vec![],
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: false,
        is_async: false,
        is_generator: false,
        is_strict: false,
    }));

    let mut count = 0;
    for_each_worker_new(&module, &mut |_| count += 1);
    assert_eq!(count, 1);
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

/// #6304 test scaffolding: a module that DEFINES `name` as an exported
/// function, so origin resolution has something real to stop on.
fn module_defining_fn(module: &str, name: &str) -> Module {
    let mut m = Module::new(module);
    m.functions.push(crate::ir::Function {
        id: 0,
        name: name.into(),
        type_params: vec![],
        params: vec![],
        return_type: Type::Any,
        body: vec![],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: true,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    });
    m.exports.push(Export::Named {
        local: name.into(),
        exported: name.into(),
    });
    m
}

/// #6304: a plain non-native named import of `imported` from `source`.
fn named_import(source: &str, imported: &str, local: &str) -> crate::ir::Import {
    crate::ir::Import {
        source: source.into(),
        specifiers: vec![crate::ir::ImportSpecifier::Named {
            imported: imported.into(),
            local: local.into(),
        }],
        is_native: false,
        module_kind: crate::ir::ModuleKind::NativeCompiled,
        resolved_path: None,
        type_only: false,
        is_dynamic: false,
        is_dynamic_target: false,
        is_deferred_require: false,
        is_adopted_require: false,
    }
}

#[test]
fn flatten_reexport_only_chunk_resolves_to_defining_module() {
    // #6304 — the canonical esbuild/bun `--splitting` shared-chunk shape:
    //
    //   // agent-VP4LHHJR.js
    //   import { run } from "./chunk-XXXX.js";
    //   export { run };
    //
    // `run` is an IMPORT binding, not a definition. Pre-fix this flattened to
    // `source_module = agent`, the driver found no local `run` there, and the
    // namespace entry degraded to an undefined-returning stub — so `ns.run`
    // came out `undefined` for the whole chunk.
    let chunk = module_defining_fn("chunk", "run");
    let mut agent = Module::new("agent");
    agent.imports.push(named_import("chunk", "run", "run"));
    agent.exports.push(Export::Named {
        local: "run".into(),
        exported: "run".into(),
    });
    let map = std::collections::HashMap::from([
        ("chunk".to_string(), chunk),
        ("agent".to_string(), agent),
    ]);
    let lookup = |s: &str| map.get(s);
    let flat = flatten_exports("agent", &lookup);
    assert_eq!(flat.len(), 1);
    assert_eq!(flat[0].name, "run");
    // The value lives in `chunk`, NOT in `agent`.
    assert_eq!(flat[0].source_module, "chunk");
    assert_eq!(flat[0].source_local, "run");
    assert_eq!(flat[0].nested_namespace_of, None);
}

#[test]
fn flatten_reexport_only_chunk_honours_import_rename() {
    // `import { run as go } from "./chunk"; export { go as run }` — the export
    // key is the consumer-visible `run`, but the binding in `chunk` is `run`
    // (the import's ORIGINAL name), not the local alias `go`.
    let chunk = module_defining_fn("chunk", "run");
    let mut agent = Module::new("agent");
    agent.imports.push(named_import("chunk", "run", "go"));
    agent.exports.push(Export::Named {
        local: "go".into(),
        exported: "run".into(),
    });
    let map = std::collections::HashMap::from([
        ("chunk".to_string(), chunk),
        ("agent".to_string(), agent),
    ]);
    let lookup = |s: &str| map.get(s);
    let flat = flatten_exports("agent", &lookup);
    assert_eq!(flat.len(), 1);
    assert_eq!(flat[0].name, "run");
    assert_eq!(flat[0].source_module, "chunk");
    assert_eq!(flat[0].source_local, "run");
}

#[test]
fn flatten_reexport_chain_reaches_ultimate_owner() {
    // A chain of forwarding chunks must land on the module that actually
    // defines the binding, not on the first hop (which would itself only
    // forward, yielding another undefined stub).
    //   outer -> mid (import+export) -> impl (definition)
    let impl_mod = module_defining_fn("impl", "run");
    let mut mid = Module::new("mid");
    mid.imports.push(named_import("impl", "run", "run"));
    mid.exports.push(Export::Named {
        local: "run".into(),
        exported: "run".into(),
    });
    let mut outer = Module::new("outer");
    outer.imports.push(named_import("mid", "run", "run"));
    outer.exports.push(Export::Named {
        local: "run".into(),
        exported: "run".into(),
    });
    let map = std::collections::HashMap::from([
        ("impl".to_string(), impl_mod),
        ("mid".to_string(), mid),
        ("outer".to_string(), outer),
    ]);
    let lookup = |s: &str| map.get(s);
    let flat = flatten_exports("outer", &lookup);
    assert_eq!(flat.len(), 1);
    assert_eq!(flat[0].source_module, "impl");
    assert_eq!(flat[0].source_local, "run");
}

#[test]
fn flatten_local_definition_still_wins_over_same_named_import() {
    // A module that DEFINES the name it exports must keep pointing at itself —
    // the redirect only fires for bindings this module does not define.
    let mut m = module_defining_fn("m", "run");
    // A same-named import would be invalid TS, but assert the definition wins
    // regardless so the redirect can never hijack a real local body.
    m.imports.push(named_import("other", "run", "run"));
    let other = module_defining_fn("other", "run");
    let map = std::collections::HashMap::from([("m".to_string(), m), ("other".to_string(), other)]);
    let lookup = |s: &str| map.get(s);
    let flat = flatten_exports("m", &lookup);
    assert_eq!(flat.len(), 1);
    assert_eq!(flat[0].source_module, "m");
    assert_eq!(flat[0].source_local, "run");
}

#[test]
fn flatten_reexport_of_native_import_is_not_redirected() {
    // `import { readFile } from "fs"; export { readFile }` — "fs" is a native
    // module with no HIR and no `perry_fn_*` symbols. Redirecting there would
    // name a module that does not exist in the graph, so the native import is
    // skipped and the entry keeps its pre-existing local shape.
    let mut m = Module::new("m");
    let mut imp = named_import("fs", "readFile", "readFile");
    imp.is_native = true;
    m.imports.push(imp);
    m.exports.push(Export::Named {
        local: "readFile".into(),
        exported: "readFile".into(),
    });
    let map = std::collections::HashMap::from([("m".to_string(), m)]);
    let lookup = |s: &str| map.get(s);
    let flat = flatten_exports("m", &lookup);
    assert_eq!(flat.len(), 1);
    assert_eq!(flat[0].source_module, "m");
    assert_eq!(flat[0].source_local, "readFile");
}

#[test]
fn flatten_reexport_binding_cycle_terminates() {
    // Two chunks that forward the same name to each other — must not loop.
    let mut a = Module::new("a");
    a.imports.push(named_import("b", "v", "v"));
    a.exports.push(Export::Named {
        local: "v".into(),
        exported: "v".into(),
    });
    let mut b = Module::new("b");
    b.imports.push(named_import("a", "v", "v"));
    b.exports.push(Export::Named {
        local: "v".into(),
        exported: "v".into(),
    });
    let map = std::collections::HashMap::from([("a".to_string(), a), ("b".to_string(), b)]);
    let lookup = |s: &str| map.get(s);
    let flat = flatten_exports("a", &lookup);
    // Terminates (no stack overflow / hang) and still yields the name.
    assert_eq!(flat.len(), 1);
    assert_eq!(flat[0].name, "v");
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
