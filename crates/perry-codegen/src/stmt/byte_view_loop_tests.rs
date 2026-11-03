use perry_hir::{types::Type, BinaryOp, CompareOp, Expr, Function, Module, Param, Stmt, UpdateOp};

/// `function inc(buf: Buffer) { for (let i = 0; i < buf.length; i++) buf[i] = (buf[i] + 1) & 255; }`
/// with `extra` appended to the loop body.
fn ir(extra: Vec<Stmt>) -> String {
    ir_labelled(extra, None)
}

/// [`ir`] with the loop labelled `label:` when given.
fn ir_labelled(extra: Vec<Stmt>, label: Option<&str>) -> String {
    let mut m = Module::new("byte_view_loop");
    let element = || Expr::Uint8ArrayGet {
        array: Box::new(Expr::LocalGet(1)),
        index: Box::new(Expr::LocalGet(2)),
    };
    let mut body = vec![Stmt::Expr(Expr::Uint8ArraySet {
        array: Box::new(Expr::LocalGet(1)),
        index: Box::new(Expr::LocalGet(2)),
        value: Box::new(Expr::Binary {
            op: BinaryOp::BitAnd,
            left: Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(element()),
                right: Box::new(Expr::Integer(1)),
            }),
            right: Box::new(Expr::Integer(255)),
        }),
    })];
    body.extend(extra);
    m.functions.push(Function {
        id: 1,
        name: "inc".into(),
        type_params: vec![],
        params: vec![Param {
            id: 1,
            name: "buf".into(),
            ty: Type::Named("Buffer".into()),
            default: None,
            decorators: vec![],
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Number,
        body: vec![
            labelled(
                label,
                Stmt::For {
                    init: Some(Box::new(Stmt::Let {
                        id: 2,
                        name: "i".into(),
                        ty: Type::Number,
                        mutable: true,
                        init: Some(Expr::Integer(0)),
                    })),
                    condition: Some(Expr::Compare {
                        op: CompareOp::Lt,
                        left: Box::new(Expr::LocalGet(2)),
                        right: Box::new(Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(1)),
                            property: "length".into(),
                            byte_offset: 0,
                            byte_end: 0,
                        }),
                    }),
                    update: Some(Expr::Update {
                        id: 2,
                        op: UpdateOp::Increment,
                        prefix: false,
                    }),
                    body,
                },
            ),
            Stmt::Return(Some(Expr::Integer(0))),
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

fn labelled(label: Option<&str>, stmt: Stmt) -> Stmt {
    match label {
        Some(label) => Stmt::Labeled {
            label: label.into(),
            body: Box::new(stmt),
        },
        None => stmt,
    }
}

/// The generic entry's text: the fast copy's blocks run from the merge
/// block to the original copy's first block.
fn copies(ir: &str) -> (&str, &str) {
    let start = ir
        .find("define double @perry_fn_byte_view_loop__inc(")
        .or_else(|| ir.find("@perry_fn_byte_view_loop__inc("))
        .expect("generic entry");
    let function = &ir[start..];
    let function = &function[..function.find("\n}\n").expect("function end")];
    let fast = function
        .find("\nfor.byte_view.merge")
        .expect("versioned loop");
    let slow = function
        .find("\nfor.byte_view_slow.")
        .expect("original copy");
    (&function[fast..slow], &function[slow..])
}

#[test]
fn admitted_byte_loop_has_no_runtime_arm() {
    let ir = ir(vec![]);
    let (fast, slow) = copies(&ir);
    for helper in ["@js_uint8array_get", "@js_uint8array_set", "u8c."] {
        assert!(!fast.contains(helper), "fast copy kept {helper}:\n{fast}");
    }
    assert!(
        fast.contains("load i8") && fast.contains("store i8"),
        "{fast}"
    );
    // The refused receiver keeps the runtime accessors.
    assert!(slow.contains("@js_uint8array_get") && slow.contains("@js_uint8array_set"));
    // One admission: the receiver proof and the named-length flag, before
    // the branch to the fast copy.
    let preheader = &ir[..ir.find("\nfor.byte_view.fast.preheader").unwrap()];
    assert!(preheader.contains("PERRY_TYPED_NAMED_PROPS_INVALIDATED"));
}

#[test]
fn a_call_in_the_body_keeps_the_ordinary_loop() {
    let ir = ir(vec![Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::LocalGet(1)),
        args: vec![],
        type_args: vec![],
        byte_offset: 0,
        byte_end: 0,
    })]);
    assert!(
        !ir.contains("for.byte_view"),
        "a call can detach the receiver"
    );
}

#[test]
fn a_non_counter_key_keeps_the_ordinary_loop() {
    let ir = ir(vec![Stmt::Expr(Expr::Uint8ArraySet {
        array: Box::new(Expr::LocalGet(1)),
        index: Box::new(Expr::Integer(0)),
        value: Box::new(Expr::Integer(1)),
    })]);
    assert!(
        !ir.contains("for.byte_view"),
        "only `P[i]` is bounded by the guard"
    );
}

/// `!N = <body>` of a numbered metadata line.
fn metadata<'a>(ir: &'a str, id: &str) -> &'a str {
    let prefix = format!("\n!{id} = ");
    let start = ir
        .find(&prefix)
        .unwrap_or_else(|| panic!("no !{id}:\n{ir}"))
        + prefix.len();
    let rest = &ir[start..];
    &rest[..rest.find('\n').unwrap_or(rest.len())]
}

/// The `!llvm.loop` ids on the latches of `text`.
fn loop_ids(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.split(", !llvm.loop !").nth(1))
        .map(|id| id.trim().to_string())
        .collect()
}

#[test]
fn the_admission_and_the_original_copy_are_marked_for_the_proofs() {
    let ir = ir_labelled(vec![], Some("same_buffer"));
    let (fast, slow) = copies(&ir);
    let region = metadata_region_line(&ir);
    // The admission branch carries the version mark.
    let branch = ir
        .lines()
        .find(|line| line.contains("label %for.byte_view.fast.preheader"))
        .expect("admission branch");
    assert!(
        branch.ends_with(", !perry.byte_view.version !{}"),
        "{branch}"
    );
    // Each copy's latch has its own loop ID naming the region; only the
    // original copy's lists the fallback mark.
    let fast_ids = loop_ids(fast);
    let slow_ids = loop_ids(slow);
    assert_eq!(fast_ids.len(), 1, "{fast}");
    assert_eq!(slow_ids.len(), 1, "{slow}");
    assert_ne!(fast_ids[0], slow_ids[0]);
    let fast_id = metadata(&ir, &fast_ids[0]);
    let slow_id = metadata(&ir, &slow_ids[0]);
    assert!(
        fast_id.starts_with(&format!("distinct !{{!{}, !{region}", fast_ids[0])),
        "{fast_id}"
    );
    assert!(
        slow_id.starts_with(&format!("distinct !{{!{}, !{region}, !", slow_ids[0])),
        "{slow_id}"
    );
    let fallback = slow_id.rsplit("!").next().unwrap().trim_end_matches('}');
    assert_eq!(metadata(&ir, fallback), "!{!\"perry.byte_view.fallback\"}");
    assert!(!fast_id.contains(&format!("!{fallback}}}")), "{fast_id}");
}

/// The id of the `perry.region` node of the `same_buffer` label.
fn metadata_region_line(ir: &str) -> String {
    let line = ir
        .lines()
        .find(|line| line.contains("!\"perry.region\"") && line.contains(".same_buffer\""))
        .unwrap_or_else(|| panic!("no region node:\n{ir}"));
    line.trim_start_matches('!')
        .split(' ')
        .next()
        .unwrap()
        .to_string()
}

#[test]
fn an_unlabelled_loop_names_no_region() {
    let ir = ir(vec![]);
    assert!(!ir.contains("perry.region"), "only a label names a region");
    // The original copy is still marked: the proofs exclude it only behind
    // the admission.
    let (_, slow) = copies(&ir);
    assert_eq!(loop_ids(slow).len(), 1, "{slow}");
}
