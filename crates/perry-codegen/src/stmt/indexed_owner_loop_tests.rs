use perry_hir::{types::Type, BinaryOp, CompareOp, Expr, Function, Module, Param, Stmt, UpdateOp};

fn get(id: u32) -> Expr {
    Expr::LocalGet(id)
}
fn index(a: u32, i: Expr) -> Expr {
    Expr::IndexGet {
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
    String::from_utf8(
        crate::compile_module(
            &m,
            crate::CompileOptions {
                emit_ir_only: true,
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
