use perry_hir::{types::Type, BinaryOp, CompareOp, Expr, Function, Module, Param, Stmt, UpdateOp};

fn get(id: u32) -> Expr {
    Expr::LocalGet(id)
}
fn index(a: u32, i: Expr) -> Expr {
    Expr::IndexGet {
        byte_offset: 0,
        byte_end: 0,
        object: Box::new(get(a)),
        index: Box::new(i),
    }
}
fn bin(op: BinaryOp, l: Expr, r: Expr) -> Expr {
    Expr::Binary {
        op,
        left: Box::new(l),
        right: Box::new(r),
    }
}
fn let_(id: u32, init: Expr, mutable: bool) -> Stmt {
    Stmt::Let {
        id,
        name: format!("v{id}"),
        ty: Type::Any,
        mutable,
        init: Some(init),
    }
}
fn ir(kind: &str, body: Vec<Stmt>) -> String {
    ir_with_facts(kind, body, Default::default())
}
fn ir_with_facts(kind: &str, body: Vec<Stmt>, facts: crate::codegen::ElisionFacts) -> String {
    let mut m = Module::new("indexed_owner_contract");
    let param = |id, name: &str, ty| Param {
        id,
        name: name.into(),
        ty,
        default: None,
        decorators: vec![],
        is_rest: false,
        arguments_object: None,
    };
    m.functions.push(Function {
        id: 1,
        name: "operate".into(),
        type_params: vec![],
        params: vec![
            param(1, "a", Type::Named(kind.into())),
            param(2, "scale", Type::Any),
            param(7, "b", Type::Named(kind.into())),
        ],
        return_type: Type::Any,
        body: vec![
            Stmt::For {
                init: Some(Box::new(let_(3, Expr::Integer(0), true))),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(get(3)),
                    right: Box::new(Expr::Integer(4)),
                }),
                update: Some(Expr::Update {
                    id: 3,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body,
            },
            Stmt::Return(Some(index(1, Expr::Integer(0)))),
        ],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: true,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    });
    for (id, f) in &facts.numeric_functions {
        let mut def = m.functions[0].clone();
        def.id = *id;
        def.name = format!("numeric_leaf_{id}");
        def.params = f
            .params
            .iter()
            .map(|id| param(*id, &format!("p{id}"), Type::Number))
            .collect();
        def.body = vec![Stmt::Return(Some(f.expression.to_hir()))];
        m.functions.push(def);
    }
    String::from_utf8(
        crate::compile_module(
            &m,
            crate::CompileOptions {
                emit_ir_only: true,
                program_facts: crate::codegen::ProgramFactsView {
                    elision: std::sync::Arc::new(facts),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .unwrap(),
    )
    .unwrap()
}
fn rmw() -> Vec<Stmt> {
    vec![
        let_(4, get(1), false),
        let_(5, get(3), false),
        Stmt::Expr(Expr::IndexSet {
            object: Box::new(get(4)),
            index: Box::new(get(5)),
            value: Box::new(bin(BinaryOp::Add, index(4, get(5)), get(2))),
        }),
    ]
}
#[test]
fn owner_and_index_guards_admit_a_call_free_rmw_region_for_numeric_widths() {
    for kind in [
        "Int8Array",
        "Uint8Array",
        "Int16Array",
        "Uint16Array",
        "Int32Array",
        "Uint32Array",
        "Float32Array",
        "Float64Array",
    ] {
        let ir = ir(kind, rmw());
        let mut entered = false;
        for function in ir.split("\ndefine") {
            let Some(start) = function.find("\nindexed.owner.fast.") else {
                continue;
            };
            let fast_label = function[start + 1..]
                .lines()
                .next()
                .unwrap()
                .trim_end_matches(':');
            assert!(
                function[..start].contains(&format!("br label %{fast_label}")),
                "{kind}: fast region must be entered"
            );
            let mut blocks = std::collections::BTreeMap::<String, String>::new();
            let mut current = String::new();
            for line in function.lines() {
                if !line.starts_with(' ') && line.ends_with(':') {
                    current = line.trim_end_matches(':').to_owned();
                } else {
                    blocks
                        .entry(current.clone())
                        .or_default()
                        .push_str(&format!("{line}\n"));
                }
            }
            let mut pending = vec![fast_label.to_owned()];
            let mut seen = std::collections::BTreeSet::new();
            let mut fast = String::new();
            while let Some(label) = pending.pop() {
                if label.starts_with("indexed.owner.exit.") || !seen.insert(label.clone()) {
                    continue;
                }
                let block = &blocks[&label];
                for edge in block.split("label %").skip(1) {
                    pending.push(
                        edge.chars()
                            .take_while(|c| c.is_alphanumeric() || *c == '.' || *c == '_')
                            .collect(),
                    );
                }
                fast.push_str(block);
            }
            assert!(
                fast.contains("load ") && fast.contains("store "),
                "{kind}: real indexed operations"
            );
            assert!(
                !fast.contains("@js_"),
                "{kind}: no semantic exits or safepoints in admitted region"
            );
            assert!(
                function.contains("for.indexed_owner_slow"),
                "complete original loop retained"
            );
            entered = true;
        }
        assert!(entered, "{kind}: admitted loop missing");
    }
}
#[test]
fn effectful_rhs_keeps_the_original_evaluation_order_lowering() {
    let mut body = rmw();
    if let Stmt::Expr(Expr::IndexSet { value, .. }) = &mut body[2] {
        *value = Box::new(bin(
            BinaryOp::Add,
            index(4, get(5)),
            Expr::Call {
                byte_end: 0,
                callee: Box::new(get(2)),
                args: vec![],
                type_args: vec![],
                byte_offset: 0,
            },
        ));
    }
    let ir = ir("Float64Array", body);
    assert!(!ir.contains("indexed.owner.fast"));
}
#[test]
fn fractional_and_unbounded_indices_do_not_receive_an_interval_proof() {
    let body = vec![Stmt::Expr(Expr::IndexSet {
        object: Box::new(get(1)),
        index: Box::new(bin(BinaryOp::Div, get(3), Expr::Integer(2))),
        value: Box::new(Expr::Integer(7)),
    })];
    assert!(!ir("Float64Array", body).contains("indexed.owner.fast"));
}

#[test]
fn disjoint_owner_scopes_have_dynamic_single_scope_declarations_and_verify() {
    let mut body = rmw();
    if let Stmt::Expr(Expr::IndexSet { value, .. }) = &mut body[2] {
        *value = Box::new(bin(BinaryOp::Add, index(4, get(5)), index(7, get(3))));
    }
    let ir = ir("Float64Array", body);
    assert!(ir.contains("call void @llvm.experimental.noalias.scope.decl(metadata"));
    assert!(ir.contains("!alias.scope !{") && ir.contains("!noalias !{"));
    let context = inkwell::context::Context::create();
    let buffer = inkwell::memory_buffer::MemoryBuffer::create_from_memory_range_copy(
        ir.as_bytes(),
        "indexed-owners",
    );
    let module = context
        .create_module_from_ir(buffer)
        .expect("owner-region LLVM parses");
    module.verify().expect("dynamic owner-region scopes verify");
}

#[test]
fn static_allocation_range_and_numeric_proofs_remove_all_admission_checks() {
    let mut facts = crate::codegen::ElisionFacts::default();
    facts.indexed_owners.insert(
        1,
        crate::codegen::FactsIndexedOwner {
            kind: 7,
            min_len: 4,
            exact_len: Some(4),
            allocations: vec!["birth:a".into()],
        },
    );
    facts.number_inputs.insert(2);
    let text = ir_with_facts("Float64Array", rmw(), facts);
    assert!(text.contains("indexed.owner.fast"));
    assert!(
        !text.contains("indexed.owner.admitted"),
        "static proof must remove the guards themselves"
    );
    assert!(
        ir("Float64Array", rmw()).contains("indexed.owner.admitted"),
        "unproven annotations still need guards"
    );
}
#[test]
fn nested_numeric_loops_keep_scalar_recurrences_in_native_slots() {
    let inner = Stmt::For {
        init: Some(Box::new(let_(8, Expr::Integer(0), true))),
        condition: Some(Expr::Compare {
            op: CompareOp::Lt,
            left: Box::new(get(8)),
            right: Box::new(Expr::Integer(4)),
        }),
        update: Some(Expr::Update {
            id: 8,
            op: UpdateOp::Increment,
            prefix: false,
        }),
        body: vec![Stmt::Expr(Expr::LocalSet(
            9,
            Box::new(bin(BinaryOp::Add, get(9), index(1, get(8)))),
        ))],
    };
    let text = ir(
        "Float64Array",
        vec![
            let_(9, Expr::Integer(0), true),
            inner,
            Stmt::Expr(Expr::IndexSet {
                object: Box::new(get(7)),
                index: Box::new(get(3)),
                value: Box::new(get(9)),
            }),
        ],
    );
    assert!(text.contains("indexed.owner.nested."));
    let context = inkwell::context::Context::create();
    let buffer = inkwell::memory_buffer::MemoryBuffer::create_from_memory_range_copy(
        text.as_bytes(),
        "nested-owner",
    );
    context
        .create_module_from_ir(buffer)
        .unwrap()
        .verify()
        .unwrap();
}

#[test]
fn certified_numeric_leaf_calls_are_emitted_without_a_call_in_the_region() {
    let mut facts = crate::codegen::ElisionFacts::default();
    facts.numeric_functions.insert(
        90,
        crate::codegen::FactsNumericFunction {
            params: vec![10, 11],
            expression: crate::codegen::FactsNumericExpr::from_hir(&bin(
                BinaryOp::Div,
                Expr::Number(1.0),
                bin(BinaryOp::Add, get(10), get(11)),
            ))
            .unwrap(),
        },
    );
    let text = ir_with_facts(
        "Float64Array",
        vec![Stmt::Expr(Expr::IndexSet {
            object: Box::new(get(1)),
            index: Box::new(get(3)),
            value: Box::new(Expr::Call {
                byte_end: 0,
                callee: Box::new(Expr::FuncRef(90)),
                args: vec![get(3), get(2)],
                type_args: vec![],
                byte_offset: 0,
            }),
        })],
        facts,
    );
    let fast = text
        .split("\nindexed.owner.fast.")
        .nth(1)
        .expect("pure call admitted")
        .split("\nindexed.owner.exit.")
        .next()
        .unwrap();
    assert!(fast.contains("fdiv double"));
    assert!(!fast.contains("call "));
}

#[test]
fn specialized_byte_stores_inside_transparent_hir_blocks_use_the_same_region() {
    let text = ir(
        "Uint8Array",
        vec![Stmt::If {
            condition: Expr::Bool(true),
            then_branch: vec![Stmt::Expr(Expr::Uint8ArraySet {
                array: Box::new(get(1)),
                index: Box::new(get(3)),
                value: Box::new(bin(BinaryOp::BitAnd, get(2), Expr::Integer(255))),
            })],
            else_branch: None,
        }],
    );
    assert!(text.contains("indexed.owner.fast"));
}

fn string_code(index: Expr) -> Expr {
    Expr::Call {
        byte_end: 0,
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(get(1)),
            property: "charCodeAt".into(),
            byte_offset: 0,
            byte_end: 0,
        }),
        args: vec![index],
        type_args: vec![],
        byte_offset: 0,
        byte_end: 0,
    }
}

#[test]
fn numeric_string_region_admits_conditional_scalar_updates_and_keeps_all_exits() {
    let body = vec![
        let_(4, string_code(get(3)), false),
        Stmt::If {
            condition: Expr::Compare {
                op: CompareOp::Eq,
                left: Box::new(get(4)),
                right: Box::new(Expr::Integer(59)),
            },
            then_branch: vec![Stmt::Expr(Expr::Update {
                id: 2,
                op: UpdateOp::Increment,
                prefix: false,
            })],
            else_branch: Some(vec![Stmt::Expr(Expr::LocalSet(
                2,
                Box::new(bin(BinaryOp::Sub, get(2), Expr::Integer(1))),
            ))]),
        },
    ];
    let ir = ir("string", body);
    assert!(ir.contains("indexed.string.storage"), "{ir}");
    assert!(ir.contains("indexed.owner.if.end"), "{ir}");
    assert!(ir.contains("indexed.owner.slow"), "{ir}");
    let first_poll = ir
        .find("call void @js_gc_loop_safepoint()")
        .expect("pending GC work must be serviced before string admission");
    let storage = ir.find("indexed.string.heap").unwrap();
    assert!(
        first_poll < storage,
        "the entry poll must precede raw string storage admission: {ir}"
    );
    let fast = ir
        .split("\nindexed.owner.fast.")
        .nth(1)
        .unwrap()
        .split("\nindexed.owner.exit.")
        .next()
        .unwrap();
    assert!(
        !fast.contains("call "),
        "raw storage stays call-free: {fast}"
    );
    assert!(
        ir.contains("PERRY_AGENT_PTRS"),
        "prototype authority must dominate admission: {ir}"
    );
    assert!(
        ir.contains("and i32") && ir.contains(", 4"),
        "raw byte provenance must be validated: {ir}"
    );
}

#[test]
fn string_region_rejects_rebinding_and_calls_before_any_body_effect() {
    for body in [
        vec![
            Stmt::If {
                condition: Expr::Compare {
                    op: CompareOp::Eq,
                    left: Box::new(string_code(get(3))),
                    right: Box::new(Expr::Integer(59)),
                },
                then_branch: vec![Stmt::Expr(Expr::LocalSet(4, Box::new(Expr::Integer(1))))],
                else_branch: None,
            },
            let_(4, Expr::Integer(0), true),
        ],
        vec![
            Stmt::Expr(Expr::LocalSet(1, Box::new(Expr::Integer(4)))),
            Stmt::Expr(string_code(get(3))),
        ],
        vec![
            Stmt::Expr(string_code(get(3))),
            Stmt::Expr(Expr::Call {
                byte_end: 0,
                callee: Box::new(get(2)),
                args: vec![],
                type_args: vec![],
                byte_offset: 0,
                byte_end: 0,
            }),
        ],
        vec![Stmt::If {
            condition: Expr::Compare {
                op: CompareOp::Gt,
                left: Box::new(string_code(get(3))),
                right: Box::new(Expr::Integer(0)),
            },
            then_branch: vec![let_(4, Expr::Integer(1), false)],
            else_branch: None,
        }],
    ] {
        assert!(!ir("string", body).contains("indexed.string.storage"));
    }
}
