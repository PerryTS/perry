use super::*;

fn function_body(source: &str, name: &str) -> Vec<Stmt> {
    let ast = perry_parser::parse_typescript(source, "record_cursor.ts").unwrap();
    let hir = perry_hir::lower_module(&ast, "record_cursor", "record_cursor.ts").unwrap();
    hir.functions
        .into_iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("function {name}"))
        .body
}

/// Every `__idx_` cursor Let of the record lowering, at any depth.
fn record_cursors(stmts: &[Stmt]) -> HashSet<u32> {
    let mut out = HashSet::new();
    fn walk(stmts: &[Stmt], out: &mut HashSet<u32>) {
        for stmt in stmts {
            match stmt {
                Stmt::Let { id, name, init, .. } => {
                    if name.starts_with("__idx_") && init.as_ref().is_some_and(is_cursor_init) {
                        out.insert(*id);
                    }
                }
                Stmt::For { init, body, .. } => {
                    if let Some(init) = init {
                        walk(std::slice::from_ref(init.as_ref()), out);
                    }
                    walk(body, out);
                }
                Stmt::Try { body, catch, finally } => {
                    walk(body, out);
                    if let Some(c) = catch {
                        walk(&c.body, out);
                    }
                    if let Some(f) = finally {
                        walk(f, out);
                    }
                }
                Stmt::If { then_branch, else_branch, .. } => {
                    walk(then_branch, out);
                    if let Some(e) = else_branch {
                        walk(e, out);
                    }
                }
                Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => walk(body, out),
                Stmt::Labeled { body, .. } => walk(std::slice::from_ref(body.as_ref()), out),
                _ => {}
            }
        }
    }
    walk(stmts, &mut out);
    out
}

#[test]
fn for_of_record_cursor_is_u32() {
    let body = function_body(
        "export function sum(a: number[]): number { let t = 0; for (const x of a) { t += x; } return t; }",
        "sum",
    );
    let cursors = record_cursors(&body);
    assert_eq!(cursors.len(), 1, "one record cursor: {body:#?}");
    assert_eq!(collect_array_record_cursor_locals(&body), cursors);
}

#[test]
fn literal_record_cursor_is_u32() {
    let body = function_body(
        "export function f(p: number, q: number): number { let t = 0; for (const x of [p, q]) { t += x; } return t; }",
        "f",
    );
    let cursors = record_cursors(&body);
    assert_eq!(cursors.len(), 1, "{body:#?}");
    assert_eq!(collect_array_record_cursor_locals(&body), cursors);
}

#[test]
fn break_continue_and_nested_records_keep_the_proof() {
    let body = function_body(
        "export function f(a: number[][]): number { let t = 0;
           for (const row of a) { if (row.length === 0) continue;
             for (const x of row) { if (x < 0) break; t += x; } }
           return t; }",
        "f",
    );
    let cursors = record_cursors(&body);
    assert_eq!(cursors.len(), 2, "{body:#?}");
    assert_eq!(collect_array_record_cursor_locals(&body), cursors);
}

fn rt(method: &str, args: Vec<Expr>) -> Expr {
    Expr::NativeMethodCall {
        module: "__perry_runtime".into(),
        class_name: None,
        object: None,
        method: method.into(),
        args,
    }
}

fn mode(id: u32) -> Expr {
    Expr::Compare {
        op: CompareOp::Eq,
        left: Box::new(Expr::LocalGet(id)),
        right: Box::new(Expr::Bool(true)),
    }
}

fn cursor_let(id: u32) -> Stmt {
    Stmt::Let {
        id,
        name: format!("__idx_{id}"),
        ty: perry_hir::types::Type::Number,
        mutable: true,
        init: Some(rt("arrayRecordIndex", vec![Expr::Integer(0)])),
    }
}

/// `For { Let 3 = idx(0); 3 < ForBound(mode 2, length, 3, ..); ForUpdate(mode 2, 3++) }`.
fn record_for(length: Expr, update_mode: u32, body: Vec<Stmt>) -> Stmt {
    Stmt::For {
        init: Some(Box::new(cursor_let(3))),
        condition: Some(Expr::Compare {
            op: CompareOp::Lt,
            left: Box::new(Expr::LocalGet(3)),
            right: Box::new(rt(
                "arrayRecordForBound",
                vec![
                    mode(2),
                    length,
                    Expr::LocalGet(3),
                    Expr::Bool(false),
                    Expr::LocalSet(4, Box::new(Expr::Number(2.0))),
                ],
            )),
        }),
        update: Some(rt(
            "arrayRecordForUpdate",
            vec![
                mode(update_mode),
                Expr::Update {
                    id: 3,
                    op: UpdateOp::Increment,
                    prefix: false,
                },
            ],
        )),
        body,
    }
}

fn record_length() -> Expr {
    rt("arrayRecordLength", vec![Expr::LocalGet(1)])
}

#[test]
fn exact_record_shape_qualifies() {
    let stmts = vec![cursor_let(3), record_for(record_length(), 2, vec![])];
    assert_eq!(collect_array_record_cursor_locals(&stmts), HashSet::from([3]));
}

#[test]
fn any_other_write_keeps_the_cursor_a_double() {
    // A body write: the update would no longer run only below the length.
    let set = vec![
        cursor_let(3),
        record_for(
            record_length(),
            2,
            vec![Stmt::Expr(Expr::LocalSet(3, Box::new(Expr::Number(4e9))))],
        ),
    ];
    assert!(collect_array_record_cursor_locals(&set).is_empty());
    // A second `++` outside the record's update.
    let bump = vec![
        cursor_let(3),
        record_for(
            record_length(),
            2,
            vec![Stmt::Expr(Expr::Update {
                id: 3,
                op: UpdateOp::Increment,
                prefix: false,
            })],
        ),
    ];
    assert!(collect_array_record_cursor_locals(&bump).is_empty());
    // A write from a nested function body.
    let closure = vec![
        cursor_let(3),
        record_for(record_length(), 2, vec![]),
        Stmt::Expr(Expr::Closure {
            func_id: 9,
            params: vec![],
            return_type: perry_hir::types::Type::Any,
            body: vec![Stmt::Expr(Expr::LocalSet(3, Box::new(Expr::Integer(-1))))],
            captures: vec![3],
            mutable_captures: vec![3],
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: true,
            is_async: false,
            is_generator: false,
            is_strict: true,
        }),
    ];
    assert!(collect_array_record_cursor_locals(&closure).is_empty());
}

#[test]
fn the_update_must_be_the_records_own() {
    // A different mode local on the update: the bound no longer gates it.
    let other_mode = vec![cursor_let(3), record_for(record_length(), 5, vec![])];
    assert!(collect_array_record_cursor_locals(&other_mode).is_empty());
    // A bound that is not a u32 length.
    let wide = vec![cursor_let(3), record_for(Expr::Integer(1 << 33), 2, vec![])];
    assert!(collect_array_record_cursor_locals(&wide).is_empty());
    let unknown = vec![cursor_let(3), record_for(Expr::LocalGet(7), 2, vec![])];
    assert!(collect_array_record_cursor_locals(&unknown).is_empty());
}

fn compile(source: &str) -> String {
    let ast = perry_parser::parse_typescript(source, "record_cursor.ts").unwrap();
    let hir = perry_hir::lower_module(&ast, "record_cursor", "record_cursor.ts").unwrap();
    String::from_utf8(
        crate::compile_module(
            &hir,
            crate::CompileOptions {
                emit_ir_only: true,
                target: Some("x86_64-unknown-linux-gnu".into()),
                ..Default::default()
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn walk_ir() -> String {
    let ir = compile(
        "class P { x: number; constructor(x: number) { this.x = x; } }
         export function walk(ps: P[]): number { let t = 0; for (const p of ps) { t += p.x; } return t; }",
    );
    ir.split("\ndefine ")
        .find(|f| f.contains("walk") && f.contains("record.update"))
        .unwrap_or_else(|| panic!("walk with a record loop:\n{ir}"))
        .to_owned()
}

/// The body of the first block whose label starts with `label.`.
fn block<'a>(function: &'a str, label: &str) -> &'a str {
    function
        .split(&format!("\n{label}."))
        .nth(1)
        .and_then(|b| b.split("\n\n").next())
        .unwrap_or_else(|| panic!("no {label} block:\n{function}"))
}

/// The record's update block steps the cursor in its u32 slot: `add nuw i32`,
/// one boxed view by `uitofp` (the expression's own value), no
/// `fadd double ..., 1.0`.
#[test]
fn record_update_steps_a_u32_slot() {
    let walk = walk_ir();
    let update = block(&walk, "record.update");
    assert!(update.contains("add nuw i32"), "{update}");
    assert_eq!(update.matches("uitofp i32").count(), 1, "{update}");
    assert!(!update.contains("fadd double"), "{update}");
}

/// The indexed head consumes the u32 cursor as an integer: the bound is an
/// `icmp ult i32` against the header's length word (the Number compare only
/// on the cold length read), and the element read indexes with the cursor
/// itself (no `fptoui` of a Number view).
#[test]
fn indexed_head_takes_the_u32_cursor() {
    let walk = walk_ir();
    let fast = block(&walk, "plen.fast");
    assert!(fast.contains("icmp ult i32") && !fast.contains("uitofp"), "{fast}");
    let merge = block(&walk, "plen.merge");
    assert!(merge.contains("phi i1") && !merge.contains("fcmp"), "{merge}");
    let indexed = block(&walk, "record.indexed");
    assert!(!indexed.contains("uitofp"), "{indexed}");
    let prefix = block(&walk, "record.read.prefix");
    assert!(!prefix.contains("fptoui"), "{prefix}");
    let get = block(&walk, "record.indexed.get");
    assert!(!get.contains("uitofp"), "{get}");
}
