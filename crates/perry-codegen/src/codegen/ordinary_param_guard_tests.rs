//! #8079 — erased ordinary-parameter annotations may optimize only behind the
//! public runtime guard. Pin the three-symbol contract and the recovered
//! string lowering so a later routing refactor cannot silently seed the
//! generic body or discard the proof-bearing clone.

use crate::{compile_module, CompileOptions};
use perry_hir::types::{ObjectType, PropertyInfo, Type};
use perry_hir::{BinaryOp, CompareOp, Expr, Function, Module, Param, Stmt, TypeAlias};
use std::collections::HashMap;

fn function_ir<'a>(ir: &'a str, marker: &str) -> &'a str {
    let start = ir
        .match_indices("define ")
        .find(|(index, _)| {
            let line_end = ir[*index..]
                .find('\n')
                .map(|offset| index + offset)
                .unwrap_or(ir.len());
            ir[*index..line_end].contains(marker)
        })
        .map(|(index, _)| index)
        .unwrap_or_else(|| panic!("missing function containing {marker}:\n{ir}"));
    let end = ir[start..]
        .find("\n}")
        .map(|offset| start + offset)
        .expect("function terminator");
    &ir[start..end]
}

#[test]
fn public_guard_routes_to_proof_clone_and_conservative_fallback() {
    let payload = Type::Object(ObjectType {
        name: Some("Payload".to_string()),
        properties: HashMap::from([(
            "label".to_string(),
            PropertyInfo {
                ty: Type::String,
                optional: false,
                readonly: false,
            },
        )]),
        property_order: Some(vec!["label".to_string()]),
        index_signature: None,
    });
    let render = Function {
        id: 1,
        name: "render".to_string(),
        type_params: Vec::new(),
        params: vec![Param {
            id: 10,
            name: "payload".to_string(),
            ty: Type::Named("Payload".to_string()),
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::String,
        body: vec![Stmt::Return(Some(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(10)),
                property: "label".to_string(),
                byte_offset: 0,
            }),
            right: Box::new(Expr::String("!".to_string())),
        }))],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    };
    let mut module = Module::new("ordinary_param_guard.ts");
    module.type_aliases.push(TypeAlias {
        id: 1,
        name: "Payload".to_string(),
        type_params: Vec::new(),
        ty: payload.clone(),
        is_exported: false,
    });
    module.functions.push(render);
    // An unknown live value nominates the declaration-guarded plan but cannot
    // provide a call-site proof. It must target the public wrapper.
    module.init.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(1)),
        args: vec![Expr::Undefined],
        type_args: Vec::new(),
        byte_offset: 0,
    }));

    // The driver aggregates aliases into CompileOptions before codegen. Mirror
    // that production boundary: Module::type_aliases is retained for HIR
    // metadata, while CrossModuleCtx resolves Named types from this map.
    let mut opts = CompileOptions {
        emit_ir_only: true,
        output_type: "executable".to_string(),
        ..Default::default()
    };
    opts.type_aliases.insert("Payload".to_string(), payload);
    let ir = String::from_utf8(compile_module(&module, opts).expect("module compiles"))
        .expect("LLVM IR is UTF-8");

    let public = function_ir(&ir, "@perry_fn_ordinary_param_guard_ts__render(");
    let specialized = function_ir(&ir, "$spec_b(");
    let generic = function_ir(&ir, "$generic(");

    assert!(public.lines().next().unwrap().contains(" noinline "));
    assert!(public.contains("call i32 @js_param_type_guard("));
    assert!(public.contains("$spec_b("));
    assert!(public.contains("$generic("));
    assert!(!generic.contains("js_param_type_guard"));
    assert!(!specialized.contains("js_param_type_guard"));
    assert!(
        specialized.contains("call double @js_string_concat_box(")
            || specialized.contains("call i64 @js_value_concat_string(")
            || specialized.contains("call i64 @js_string_concat_value("),
        "the successful clone must consume the guarded string field proof:\n{specialized}"
    );
    assert!(!specialized.contains("js_dynamic_string_or_number_add"));
    // Keep #8033 intact: declaration annotations may still improve ordinary
    // generic lowering. The safety boundary pinned here is that only the
    // successful clone receives entry proofs, while the fallback contains no
    // guard-derived facts or recursive guard call.
}

#[test]
fn nonsuspending_async_function_needs_no_direct_call_site_for_its_guarded_clone() {
    // An async body with no `await` runs to completion synchronously, so the
    // entry guard still describes the live arguments when the body reads them.
    // No direct call site is required: the public wrapper is the route.
    //
    // The parameters are PRIMITIVES. `guard_blocked` (see `compile_module`)
    // refuses a descriptor proof for a reference-typed parameter in a body that
    // can reach unknown code, and `lookup.has(...)` is such a reach — the third
    // function below pins exactly that, so this fixture stays a test of the
    // async rule instead of silently becoming a test of the generic path.
    let payload = Type::Object(ObjectType {
        name: Some("Payload".to_string()),
        properties: HashMap::from([(
            "label".to_string(),
            PropertyInfo {
                ty: Type::String,
                optional: false,
                readonly: false,
            },
        )]),
        property_order: Some(vec!["label".to_string()]),
        index_signature: None,
    });
    let map_type = Type::Generic {
        base: "Map".to_string(),
        type_args: vec![Type::String, Type::Number],
    };
    let render = Function {
        id: 21,
        name: "renderAsync".to_string(),
        type_params: Vec::new(),
        params: vec![
            Param {
                id: 210,
                name: "label".to_string(),
                ty: Type::String,
                default: None,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            },
            Param {
                id: 211,
                name: "weight".to_string(),
                ty: Type::Number,
                default: None,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            },
        ],
        return_type: Type::Boolean,
        body: vec![
            Stmt::Let {
                id: 212,
                name: "lookup".to_string(),
                ty: map_type,
                mutable: false,
                init: Some(Expr::MapNew),
            },
            Stmt::Expr(Expr::MapSet {
                map: Box::new(Expr::LocalGet(212)),
                key: Box::new(Expr::LocalGet(210)),
                value: Box::new(Expr::LocalGet(211)),
            }),
            Stmt::Return(Some(Expr::MapHas {
                map: Box::new(Expr::LocalGet(212)),
                key: Box::new(Expr::LocalGet(210)),
            })),
        ],
        is_async: true,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    };
    let mut module = Module::new("ordinary_param_guard_async.ts");
    module.type_aliases.push(TypeAlias {
        id: 1,
        name: "Payload".to_string(),
        type_params: Vec::new(),
        ty: payload.clone(),
        is_exported: false,
    });
    module.functions.push(render);
    module.functions.push(Function {
        id: 22,
        name: "renderAfterAwait".to_string(),
        type_params: Vec::new(),
        params: vec![Param {
            id: 220,
            name: "payload".to_string(),
            ty: Type::Named("Payload".to_string()),
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::String,
        body: vec![Stmt::Return(Some(Expr::Await(Box::new(
            Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(220)),
                property: "label".to_string(),
                byte_offset: 0,
            },
        ))))],
        is_async: true,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    // The discriminating negative for the primitive-parameter choice above: the
    // SAME body shape with a reference-typed parameter gets no clone at all,
    // because a call can reach that object through an alias the caller arranged
    // before entry. If that rule is ever weakened, this row goes red rather
    // than the fixture above silently starting to measure something else.
    module.functions.push(Function {
        id: 23,
        name: "renderReferenceParam".to_string(),
        type_params: Vec::new(),
        params: vec![Param {
            id: 230,
            name: "payload".to_string(),
            ty: Type::Named("Payload".to_string()),
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Boolean,
        body: vec![
            Stmt::Let {
                id: 231,
                name: "lookup".to_string(),
                ty: Type::Generic {
                    base: "Map".to_string(),
                    type_args: vec![Type::String, Type::Number],
                },
                mutable: false,
                init: Some(Expr::MapNew),
            },
            Stmt::Return(Some(Expr::MapHas {
                map: Box::new(Expr::LocalGet(231)),
                key: Box::new(Expr::PropertyGet {
                    object: Box::new(Expr::LocalGet(230)),
                    property: "label".to_string(),
                    byte_offset: 0,
                }),
            })),
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

    let mut opts = CompileOptions {
        emit_ir_only: true,
        output_type: "executable".to_string(),
        ..Default::default()
    };
    opts.type_aliases.insert("Payload".to_string(), payload);
    let ir = String::from_utf8(compile_module(&module, opts).expect("module compiles"))
        .expect("LLVM IR is UTF-8");
    let public = function_ir(&ir, "@perry_fn_ordinary_param_guard_async_ts__renderAsync(");
    assert!(public.contains("call i32 @js_param_type_guard("));
    assert_eq!(public.matches("call i32 @js_param_type_guard(").count(), 2);
    assert!(public.contains("$spec_b_b("));
    assert!(public.contains("$generic("));
    let specialized = function_ir(&ir, "renderAsync$spec_b_b(");
    let generic = function_ir(&ir, "renderAsync$generic(");
    assert!(specialized.contains("@js_map_has_string_key("));
    assert!(!specialized.contains("@js_map_has("));
    assert!(generic.contains("@js_map_has("));
    assert!(!generic.contains("@js_map_has_string_key("));

    let suspended = function_ir(
        &ir,
        "@perry_fn_ordinary_param_guard_async_ts__renderAfterAwait(",
    );
    assert!(!suspended.contains("js_param_type_guard"));
    assert!(!suspended.contains("$spec_"));

    let reference_param = function_ir(
        &ir,
        "@perry_fn_ordinary_param_guard_async_ts__renderReferenceParam(",
    );
    assert!(
        !ir.contains("renderReferenceParam$spec_") && !ir.contains("renderReferenceParam$generic"),
        "a reference parameter in a body that can reach unknown code must not be guarded:\n{ir}"
    );
    assert!(
        !reference_param.contains("js_param_type_guard")
            && reference_param.contains("@js_map_has("),
        "the unguarded body must keep the generic key lowering:\n{reference_param}"
    );
}

#[test]
fn guarded_discriminant_branch_narrows_a_union_parameter_inside_the_clone() {
    // Renamed from `guarded_discriminant_branch_routes_recursive_field_to_clone`.
    // The routing half of that name described a RECURSIVE union walk, which
    // `guard_blocked` no longer admits: a call in the body can reach the
    // guarded object through an alias the caller arranged before entry, so a
    // reference-typed parameter cannot keep a descriptor proof across it. The
    // narrowing machinery it was really exercising survives on a call-free
    // body, and the recursive shape is kept below as the negative that pins
    // the rule.
    let node = Type::Union(vec![
        Type::Object(ObjectType {
            name: None,
            properties: HashMap::from([
                (
                    "kind".to_string(),
                    PropertyInfo {
                        ty: Type::StringLiteral("num".to_string()),
                        optional: false,
                        readonly: false,
                    },
                ),
                (
                    "num".to_string(),
                    PropertyInfo {
                        ty: Type::Number,
                        optional: false,
                        readonly: false,
                    },
                ),
            ]),
            property_order: Some(vec!["kind".to_string(), "num".to_string()]),
            index_signature: None,
        }),
        Type::Object(ObjectType {
            name: None,
            properties: HashMap::from([
                (
                    "kind".to_string(),
                    PropertyInfo {
                        ty: Type::StringLiteral("bin".to_string()),
                        optional: false,
                        readonly: false,
                    },
                ),
                (
                    "left".to_string(),
                    PropertyInfo {
                        ty: Type::Named("Node".to_string()),
                        optional: false,
                        readonly: false,
                    },
                ),
            ]),
            property_order: Some(vec!["kind".to_string(), "left".to_string()]),
            index_signature: None,
        }),
    ]);
    fn discriminant_let(id: u32, owner: u32) -> Stmt {
        Stmt::Let {
            id,
            name: "kind".to_string(),
            ty: Type::Any,
            mutable: false,
            init: Some(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(owner)),
                property: "kind".to_string(),
                byte_offset: 0,
            }),
        }
    }
    let eval = Function {
        id: 31,
        name: "evalNode".to_string(),
        type_params: Vec::new(),
        params: vec![Param {
            id: 310,
            name: "node".to_string(),
            ty: Type::Named("Node".to_string()),
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Number,
        body: vec![
            discriminant_let(311, 310),
            // The arm returns, so its complement dominates the statement that
            // follows. This pins the control-flow merge that interpreter-style
            // chains of discriminator checks rely on.
            Stmt::If {
                condition: Expr::Compare {
                    op: CompareOp::Eq,
                    left: Box::new(Expr::LocalGet(311)),
                    right: Box::new(Expr::String("num".to_string())),
                },
                then_branch: vec![Stmt::Return(Some(Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(310)),
                        property: "num".to_string(),
                        byte_offset: 0,
                    }),
                    right: Box::new(Expr::Number(1.0)),
                }))],
                else_branch: None,
            },
            Stmt::Return(Some(Expr::Integer(0))),
        ],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    };
    // Same union, same discriminant chain, but the "bin" arm recurses. The
    // call is what removes the clone.
    let eval_recursive = Function {
        id: 32,
        name: "evalRecursive".to_string(),
        type_params: Vec::new(),
        params: vec![Param {
            id: 320,
            name: "node".to_string(),
            ty: Type::Named("Node".to_string()),
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Number,
        body: vec![
            discriminant_let(321, 320),
            Stmt::If {
                condition: Expr::Compare {
                    op: CompareOp::Eq,
                    left: Box::new(Expr::LocalGet(321)),
                    right: Box::new(Expr::String("bin".to_string())),
                },
                then_branch: vec![Stmt::Return(Some(Expr::Call {
                    callee: Box::new(Expr::FuncRef(32)),
                    args: vec![Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(320)),
                        property: "left".to_string(),
                        byte_offset: 0,
                    }],
                    type_args: Vec::new(),
                    byte_offset: 0,
                }))],
                else_branch: None,
            },
            Stmt::Return(Some(Expr::Integer(0))),
        ],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    };
    let mut module = Module::new("recursive_guard_narrowing.ts");
    module.type_aliases.push(TypeAlias {
        id: 31,
        name: "Node".to_string(),
        type_params: Vec::new(),
        ty: node.clone(),
        is_exported: false,
    });
    module.functions.push(eval);
    module.functions.push(eval_recursive);
    module.init.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(31)),
        args: vec![Expr::Undefined],
        type_args: Vec::new(),
        byte_offset: 0,
    }));

    let mut opts = CompileOptions {
        emit_ir_only: true,
        output_type: "executable".to_string(),
        ..Default::default()
    };
    opts.type_aliases.insert("Node".to_string(), node);
    let ir = String::from_utf8(compile_module(&module, opts).expect("module compiles"))
        .expect("LLVM IR is UTF-8");
    let public = function_ir(&ir, "@perry_fn_recursive_guard_narrowing_ts__evalNode(");
    assert!(public.contains("call i32 @js_param_type_guard("));
    assert!(public.contains("evalNode$spec_b("));
    assert!(public.contains("evalNode$generic("));

    let specialized = function_ir(&ir, "evalNode$spec_b(");
    let generic = function_ir(&ir, "evalNode$generic(");
    // Inside the clone the entry guard proved `Node`, so `kind === "num"`
    // narrows the union to its first arm and `node.num` is a proven number:
    // the add lowers to a raw `fadd`. The generic body has no such proof and
    // must keep the dynamic add — that pair is the whole subject.
    assert!(
        specialized.contains("fadd double")
            && !specialized.contains("js_dynamic_string_or_number_add"),
        "the guarded clone should narrow the discriminated union and add raw:\n{specialized}"
    );
    assert!(
        generic.contains("call double @js_dynamic_string_or_number_add(")
            && !generic.contains("fadd double"),
        "the unproven body must keep the dynamic add:\n{generic}"
    );

    // The negative that pins the rule: same union, same discriminant chain,
    // one recursive call — and the clone is gone. Without this, weakening
    // `guard_blocked` would go unnoticed here.
    assert!(
        !ir.contains("evalRecursive$spec_") && !ir.contains("evalRecursive$generic"),
        "a reference parameter must not keep a descriptor proof across a call:\n{ir}"
    );
    let recursive = function_ir(
        &ir,
        "@perry_fn_recursive_guard_narrowing_ts__evalRecursive(",
    );
    assert!(
        !recursive.contains("js_param_type_guard"),
        "the recursive walker must stay on the unguarded body:\n{recursive}"
    );
}
