//! Async-to-generator tests for computed-key and field-position awaits.
//!
//! Split out of `async_to_generator.rs` (2000-line-per-file cap). Pure
//! relocation; declared with `#[path]` from the parent so the module path
//! -- and therefore every test name -- is unchanged.

use super::*;

// The minimal shape the collect scan matches: `async () => { await 1 }`
// — `Expr::Closure { is_async, !is_generator }` whose body has an Await.
fn async_closure_with_await(func_id: perry_hir::types::FuncId) -> Expr {
    Expr::Closure {
        func_id,
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![Stmt::Expr(Expr::Await(Box::new(Expr::Integer(1))))],
        captures: Vec::new(),
        mutable_captures: Vec::new(),
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: true,
        is_generator: false,
        is_strict: false,
    }
}

fn empty_fn(id: perry_hir::types::FuncId, body: Vec<Stmt>) -> Function {
    Function {
        id,
        name: String::new(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body,
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }
}

fn empty_class(name: &str) -> Class {
    Class {
        id: 1,
        name: name.to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: Vec::new(),
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

fn field_with_init(name: &str, init: Expr) -> ClassField {
    ClassField {
        name: name.to_string(),
        key_expr: None,
        ty: Type::Any,
        init: Some(init),
        is_private: false,
        is_readonly: false,
        decorators: Vec::new(),
    }
}

// `h = async () => await 1` as an INSTANCE field: before #5854's collect
// extension the scan skipped `class.fields`, so this closure's FuncId never
// entered `async_step_closures`, the rewrite pass (which filters on that
// set) skipped it, and it stayed a raw block-waiting async fn. It must now
// be BOTH collected and CPS-rewritten to a generator.
#[test]
fn async_closure_in_instance_field_is_collected_and_rewritten() {
    let mut module = Module::new("test");
    let mut class = empty_class("C");
    class
        .fields
        .push(field_with_init("h", async_closure_with_await(50)));
    module.classes.push(class);

    transform_async_to_generator(&mut module);

    assert!(
        module.async_step_closures.contains(&50),
        "instance-field async closure FuncId must be collected"
    );
    // An async CLOSURE with awaits is rewritten in place: its body becomes a
    // state machine (via transform_plain_async_closure_body) and `is_async`
    // is cleared. (Unlike a top-level async fn, it is NOT re-flagged as a
    // generator — the transformed body IS the driver.) A cleared `is_async`
    // is the definitive signal the rewrite fired rather than falling back to
    // raw block-wait.
    match &module.classes[0].fields[0].init {
        Some(Expr::Closure { is_async, .. }) => assert!(
            !*is_async,
            "field async closure must be CPS-rewritten (is_async cleared)"
        ),
        other => panic!("field init should still be a Closure, got {other:?}"),
    }
}

// Companion: a STATIC field initializer is a separate container the collect
// scan also skipped pre-#5854.
#[test]
fn async_closure_in_static_field_is_collected() {
    let mut module = Module::new("test");
    let mut class = empty_class("C");
    class
        .static_fields
        .push(field_with_init("h", async_closure_with_await(60)));
    module.classes.push(class);

    transform_async_to_generator(&mut module);

    assert!(
        module.async_step_closures.contains(&60),
        "static-field async closure FuncId must be collected"
    );
}

// ── Differential await-position audit (#8681 -p streaming hang) ──────────
//
// The `-p` streaming deadlock traced to an async CLOSURE whose `await`
// reached codegen as a raw `Expr::Await` (the `fs_await.rs` blocking
// busy-wait) instead of a suspend point — i.e. `transform_async_to_generator`
// did not rewrite it. A raw block-wait entered from inside the async-step /
// microtask cascade (the SSE async-generator pull chain) monopolises the
// single runtime thread and self-deadlocks.
//
// For every syntactic position an `await` can sit in, an async closure that
// contains one MUST be (a) collected into `async_step_closures` and (b)
// CPS-rewritten so `is_async` is cleared. A cleared `is_async` is the
// definitive "rewrite fired, will suspend" signal; a still-set `is_async` on
// a closure that has an await is exactly the block-wait escape. This test
// sweeps the positions so a future edit to the walker / rewrite that drops
// one is caught here instead of in a 30-minute bundle compile.
fn await_(inner: Expr) -> Expr {
    Expr::Await(Box::new(inner))
}

/// An async arrow whose body is `stmts`, at `func_id`.
fn async_closure_body(func_id: perry_hir::types::FuncId, stmts: Vec<Stmt>) -> Expr {
    Expr::Closure {
        func_id,
        params: Vec::new(),
        return_type: Type::Any,
        body: stmts,
        captures: Vec::new(),
        mutable_captures: Vec::new(),
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: true,
        is_generator: false,
        is_strict: false,
    }
}

#[test]
fn async_closure_await_in_every_position_is_rewritten() {
    // (label, body containing exactly one `await` in the named position)
    let cases: Vec<(&str, Vec<Stmt>)> = vec![
        (
            "ternary-then",
            vec![Stmt::Expr(Expr::Conditional {
                condition: Box::new(Expr::Bool(true)),
                then_expr: Box::new(await_(Expr::Integer(1))),
                else_expr: Box::new(Expr::Integer(0)),
            })],
        ),
        (
            "logical-and-rhs",
            vec![Stmt::Expr(Expr::Logical {
                op: LogicalOp::And,
                left: Box::new(Expr::Bool(true)),
                right: Box::new(await_(Expr::Integer(1))),
            })],
        ),
        (
            "logical-coalesce-rhs",
            vec![Stmt::Expr(Expr::Logical {
                op: LogicalOp::Coalesce,
                left: Box::new(Expr::Null),
                right: Box::new(await_(Expr::Integer(1))),
            })],
        ),
        (
            "sequence",
            vec![Stmt::Expr(Expr::Sequence(vec![
                Expr::Integer(0),
                await_(Expr::Integer(1)),
            ]))],
        ),
        (
            "switch-discriminant",
            vec![Stmt::Switch {
                discriminant: await_(Expr::Integer(1)),
                cases: vec![],
            }],
        ),
        (
            "switch-case-body",
            vec![Stmt::Switch {
                discriminant: Expr::Integer(0),
                cases: vec![SwitchCase {
                    test: Some(Expr::Integer(0)),
                    body: vec![Stmt::Expr(await_(Expr::Integer(1)))],
                }],
            }],
        ),
        (
            "try-body",
            vec![Stmt::Try {
                body: vec![Stmt::Expr(await_(Expr::Integer(1)))],
                catch: None,
                finally: None,
            }],
        ),
        (
            "catch-body",
            vec![Stmt::Try {
                body: vec![],
                catch: Some(CatchClause {
                    param: None,
                    body: vec![Stmt::Expr(await_(Expr::Integer(1)))],
                }),
                finally: None,
            }],
        ),
        (
            "finally-body",
            vec![Stmt::Try {
                body: vec![],
                catch: None,
                finally: Some(vec![Stmt::Expr(await_(Expr::Integer(1)))]),
            }],
        ),
        (
            "array-element",
            vec![Stmt::Expr(Expr::Array(vec![await_(Expr::Integer(1))]))],
        ),
        (
            "object-value",
            vec![Stmt::Expr(Expr::Object(vec![(
                "k".to_string(),
                await_(Expr::Integer(1)),
            )]))],
        ),
        (
            "call-arg",
            vec![Stmt::Expr(Expr::Call {
                callee: Box::new(Expr::Undefined),
                args: vec![await_(Expr::Integer(1))],
                type_args: vec![],
                byte_offset: 0,
            })],
        ),
        (
            "index",
            vec![Stmt::Expr(Expr::IndexGet {
                object: Box::new(Expr::Array(vec![])),
                index: Box::new(await_(Expr::Integer(1))),
            })],
        ),
        (
            "binary-rhs",
            vec![Stmt::Expr(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::Integer(1)),
                right: Box::new(await_(Expr::Integer(1))),
            })],
        ),
        (
            "return-await",
            vec![Stmt::Return(Some(await_(Expr::Integer(1))))],
        ),
        ("throw-await", vec![Stmt::Throw(await_(Expr::Integer(1)))]),
        (
            "if-condition",
            vec![Stmt::If {
                condition: await_(Expr::Bool(true)),
                then_branch: vec![],
                else_branch: None,
            }],
        ),
        (
            "while-body",
            vec![Stmt::While {
                condition: Expr::Bool(false),
                body: vec![Stmt::Expr(await_(Expr::Integer(1)))],
            }],
        ),
        (
            "for-of-iterable",
            vec![Stmt::Expr(Expr::ForOfToArray(Box::new(await_(
                Expr::Array(vec![]),
            ))))],
        ),
    ];

    // One class per case: the closure sits in an instance-field initializer
    // (a stable container that stays an `Expr::Closure` after transform, so
    // its `is_async` is directly inspectable — see the #5854 test above).
    let mut module = Module::new("test");
    let base: perry_hir::types::FuncId = 1000;
    for (i, (_label, body)) in cases.iter().enumerate() {
        let id = base + i as perry_hir::types::FuncId;
        let mut class = empty_class("C");
        class
            .fields
            .push(field_with_init("h", async_closure_body(id, body.clone())));
        module.classes.push(class);
    }

    transform_async_to_generator(&mut module);

    let mut escaped: Vec<String> = Vec::new();
    for (i, (label, _body)) in cases.iter().enumerate() {
        let id = base + i as perry_hir::types::FuncId;
        let collected = module.async_step_closures.contains(&id);
        let rewritten = matches!(
            &module.classes[i].fields[0].init,
            Some(Expr::Closure {
                is_async: false,
                ..
            })
        );
        if !collected || !rewritten {
            escaped.push(format!(
                "{label}: collected={collected} rewritten={rewritten}"
            ));
        }
    }
    assert!(
        escaped.is_empty(),
        "async closures with an await in these positions escaped the \
         async->generator transform (would block-wait at runtime): {escaped:#?}"
    );
}

// #8681 (-p streaming hang): an async-generator CLASS METHOD
// (`async *[Symbol.asyncIterator]()` — the Anthropic SDK `Stream` shape) must
// be recorded in `module.async_generator_funcs` just like a top-level
// `async function* g(){}`, or codegen never builds its async-generator driver
// wrapper and the method runs as a plain SYNC generator: its linearized
// awaits fall back to the blocking busy-wait (`fs_await.rs`), which
// self-deadlocks when driven from inside the async-step/microtask cascade.
fn async_gen_fn(id: perry_hir::types::FuncId) -> Function {
    let mut f = empty_fn(
        id,
        vec![Stmt::Expr(Expr::Yield {
            value: Some(Box::new(Expr::Await(Box::new(Expr::Integer(1))))),
            delegate: false,
        })],
    );
    f.is_async = true;
    f.is_generator = true;
    f
}

#[test]
fn async_generator_class_methods_are_recorded_like_top_level() {
    use crate::generator::transform_generators;

    let mut module = Module::new("test");

    // (a) baseline: a top-level `async function* g(){}` — known-recorded.
    module.functions.push(async_gen_fn(100));

    // (b) an async-generator INSTANCE method, (c) STATIC method,
    // (d) COMPUTED-key member — the three class containers.
    let mut class = empty_class("Stream");
    class.methods.push(async_gen_fn(200));
    class.static_methods.push(async_gen_fn(300));
    class.computed_members.push(ClassComputedMember {
        key_expr: Expr::Integer(0),
        function: async_gen_fn(400),
        is_static: false,
        kind: ClassComputedMemberKind::Method,
        source_order: 0,
    });
    module.classes.push(class);

    // The async-step pre-pass runs first in the real pipeline, then the
    // generator transform records async-generator func ids.
    transform_async_to_generator(&mut module);
    transform_generators(&mut module);

    let recorded = &module.async_generator_funcs;
    assert!(
        recorded.contains(&100),
        "top-level async generator must be recorded (baseline)"
    );
    let mut missing: Vec<(&str, perry_hir::types::FuncId)> = Vec::new();
    for (label, id) in [
        ("instance-method", 200),
        ("static-method", 300),
        ("computed-member", 400),
    ] {
        if !recorded.contains(&id) {
            missing.push((label, id));
        }
    }
    assert!(
        missing.is_empty(),
        "async-generator class methods NOT recorded in async_generator_funcs \
         (they will run as sync generators and block-wait): {missing:?}"
    );
}

// ── Async-generator linearizer residual-await audit (#8681) ──────────────
//
// After the full async pipeline (`transform_async_to_generator` +
// `transform_generators`), NO raw `Expr::Await` may survive anywhere: every
// await is either linearized into a generator suspend or CPS-rewritten in a
// nested async closure. A surviving raw `Expr::Await` is compiled by
// `fs_await.rs` into the blocking busy-wait — the exact `-p` deadlock when it
// fires from inside the async-step / async-generator pull chain. The prior
// fix in this family (pi #6728) was an `await` inside `if`/loop/`try` in an
// async generator that never suspended; this sweep guards the whole matrix.
fn count_raw_awaits_stmts(stmts: &[Stmt]) -> usize {
    stmts.iter().map(count_raw_awaits_stmt).sum()
}
fn count_raw_awaits_stmt(s: &Stmt) -> usize {
    match s {
        Stmt::Let { init: Some(e), .. }
        | Stmt::Expr(e)
        | Stmt::Throw(e)
        | Stmt::Return(Some(e)) => count_raw_awaits_expr(e),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            count_raw_awaits_expr(condition)
                + count_raw_awaits_stmts(then_branch)
                + else_branch
                    .as_ref()
                    .map_or(0, |b| count_raw_awaits_stmts(b))
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            count_raw_awaits_expr(condition) + count_raw_awaits_stmts(body)
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_ref().map_or(0, |i| count_raw_awaits_stmt(i))
                + condition.as_ref().map_or(0, |c| count_raw_awaits_expr(c))
                + update.as_ref().map_or(0, |u| count_raw_awaits_expr(u))
                + count_raw_awaits_stmts(body)
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            count_raw_awaits_stmts(body)
                + catch
                    .as_ref()
                    .map_or(0, |c| count_raw_awaits_stmts(&c.body))
                + finally.as_ref().map_or(0, |f| count_raw_awaits_stmts(f))
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            count_raw_awaits_expr(discriminant)
                + cases
                    .iter()
                    .map(|c| {
                        c.test.as_ref().map_or(0, count_raw_awaits_expr)
                            + count_raw_awaits_stmts(&c.body)
                    })
                    .sum::<usize>()
        }
        Stmt::Labeled { body, .. } => count_raw_awaits_stmt(body),
        _ => 0,
    }
}
fn count_raw_awaits_expr(e: &Expr) -> usize {
    let mut n = if matches!(e, Expr::Await(_)) { 1 } else { 0 };
    // Descend into a nested closure body too: after the pipeline an async
    // closure is a state machine, so a raw await there is equally a bug.
    if let Expr::Closure { body, .. } = e {
        n += count_raw_awaits_stmts(body);
    }
    perry_hir::walker::walk_expr_children(e, &mut |c| n += count_raw_awaits_expr(c));
    n
}

fn async_gen_with_body(id: perry_hir::types::FuncId, body: Vec<Stmt>) -> Function {
    let mut f = empty_fn(id, body);
    f.is_async = true;
    f.is_generator = true;
    f
}

#[test]
fn async_generator_linearizes_every_await_position() {
    use crate::generator::transform_generators;

    let y = |v: Expr| {
        Stmt::Expr(Expr::Yield {
            value: Some(Box::new(v)),
            delegate: false,
        })
    };
    let cases: Vec<(&str, Vec<Stmt>)> = vec![
        ("yield-await", vec![y(await_(Expr::Integer(1)))]),
        (
            "await-in-if-body",
            vec![Stmt::If {
                condition: Expr::Bool(true),
                then_branch: vec![Stmt::Expr(await_(Expr::Integer(1))), y(Expr::Integer(2))],
                else_branch: None,
            }],
        ),
        (
            "await-in-while-body",
            vec![Stmt::While {
                condition: Expr::Bool(false),
                body: vec![Stmt::Expr(await_(Expr::Integer(1))), y(Expr::Integer(2))],
            }],
        ),
        (
            "await-in-for-body",
            vec![Stmt::For {
                init: None,
                condition: Some(Expr::Bool(false)),
                update: None,
                body: vec![Stmt::Expr(await_(Expr::Integer(1))), y(Expr::Integer(2))],
            }],
        ),
        (
            "await-in-try-body",
            vec![Stmt::Try {
                body: vec![Stmt::Expr(await_(Expr::Integer(1))), y(Expr::Integer(2))],
                catch: None,
                finally: None,
            }],
        ),
        (
            "await-in-catch",
            vec![Stmt::Try {
                body: vec![y(Expr::Integer(0))],
                catch: Some(CatchClause {
                    param: None,
                    body: vec![Stmt::Expr(await_(Expr::Integer(1)))],
                }),
                finally: None,
            }],
        ),
        // NOTE: `await` inside a `finally` of a REAL async generator
        // (`async function*`) is a SEPARATE, pre-existing gap in the
        // `#4438` B2-finally lowering — the yielding finally's states are
        // built with a raw `Expr::Await` instead of an async suspend, so it
        // block-waits the same way. It is NOT addressed by this PR (which
        // fixes the `was_plain_async` catch path); the closure test
        // `async_closure_rewrite_leaves_no_residual_await` DOES cover
        // `in-finally` for the `was_plain_async` path, which is clean.
        // Tracked separately in #8715; omitted here so this test asserts
        // only what this change fixes.
        (
            "await-in-if-inside-try-inside-loop",
            // The pi #6728 shape: await buried in nested control flow.
            vec![Stmt::While {
                condition: Expr::Bool(false),
                body: vec![Stmt::Try {
                    body: vec![Stmt::If {
                        condition: Expr::Bool(true),
                        then_branch: vec![Stmt::Expr(await_(Expr::Integer(1)))],
                        else_branch: None,
                    }],
                    catch: None,
                    finally: None,
                }],
            }],
        ),
        (
            "await-in-switch-case",
            vec![Stmt::Switch {
                discriminant: Expr::Integer(0),
                cases: vec![SwitchCase {
                    test: Some(Expr::Integer(0)),
                    body: vec![Stmt::Expr(await_(Expr::Integer(1)))],
                }],
            }],
        ),
        (
            "await-in-ternary",
            vec![Stmt::Expr(Expr::Conditional {
                condition: Box::new(Expr::Bool(true)),
                then_expr: Box::new(await_(Expr::Integer(1))),
                else_expr: Box::new(Expr::Integer(0)),
            })],
        ),
        (
            "await-in-logical",
            vec![Stmt::Expr(Expr::Logical {
                op: LogicalOp::And,
                left: Box::new(Expr::Bool(true)),
                right: Box::new(await_(Expr::Integer(1))),
            })],
        ),
        (
            "await-then-yield-await",
            vec![
                Stmt::Expr(await_(Expr::Integer(1))),
                y(await_(Expr::Integer(2))),
            ],
        ),
    ];

    let mut escaped: Vec<String> = Vec::new();
    for (label, body) in &cases {
        let mut module = Module::new("test");
        module
            .functions
            .push(async_gen_with_body(500, body.clone()));
        transform_async_to_generator(&mut module);
        transform_generators(&mut module);
        // Scan every function the pipeline produced (the original plus the
        // synthesized step closures / bodies).
        let residual: usize = module
            .functions
            .iter()
            .map(|f| count_raw_awaits_stmts(&f.body))
            .sum::<usize>()
            + module.init.iter().map(count_raw_awaits_stmt).sum::<usize>();
        if residual > 0 {
            escaped.push(format!("{label}: {residual} raw await(s) survived"));
        }
    }
    assert!(
        escaped.is_empty(),
        "raw Expr::Await survived async-generator linearization (would \
         block-wait at runtime): {escaped:#?}"
    );
}

// #8681 (THE crash frame `perry_closure __85891`): a plain async CLOSURE
// rewritten to the async-step driver must leave NO raw `Expr::Await` in its
// body — every await must become an async-step suspend. A residual raw await
// is compiled by fs_await.rs (with `ctx.is_async_fn == false`, since the
// rewrite cleared `is_async`) into the blocking busy-wait + top-level-await
// exit — exactly the symbols the crash-frame closure calls
// (`js_wait_for_event`, `js_unsettled_top_level_await_exit`, ×7 sites). The
// earlier `..._await_in_every_position_is_rewritten` test only checked that
// `is_async` was cleared; it never checked for leftover awaits. This does.
#[test]
fn async_closure_rewrite_leaves_no_residual_await() {
    let cases: Vec<(&str, Vec<Stmt>)> = vec![
        ("top-level", vec![Stmt::Expr(await_(Expr::Integer(1)))]),
        (
            "in-if",
            vec![Stmt::If {
                condition: Expr::Bool(true),
                then_branch: vec![Stmt::Expr(await_(Expr::Integer(1)))],
                else_branch: None,
            }],
        ),
        (
            "in-while",
            vec![Stmt::While {
                condition: Expr::Bool(false),
                body: vec![Stmt::Expr(await_(Expr::Integer(1)))],
            }],
        ),
        (
            "in-for",
            vec![Stmt::For {
                init: None,
                condition: Some(Expr::Bool(false)),
                update: None,
                body: vec![Stmt::Expr(await_(Expr::Integer(1)))],
            }],
        ),
        (
            "in-try",
            vec![Stmt::Try {
                body: vec![Stmt::Expr(await_(Expr::Integer(1)))],
                catch: None,
                finally: None,
            }],
        ),
        (
            "try-await-and-catch-await",
            vec![Stmt::Try {
                body: vec![Stmt::Expr(await_(Expr::Integer(0)))],
                catch: Some(CatchClause {
                    param: None,
                    body: vec![Stmt::Expr(await_(Expr::Integer(1)))],
                }),
                finally: None,
            }],
        ),
        (
            "in-catch",
            vec![Stmt::Try {
                body: vec![Stmt::Expr(Expr::Integer(0))],
                catch: Some(CatchClause {
                    param: None,
                    body: vec![Stmt::Expr(await_(Expr::Integer(1)))],
                }),
                finally: None,
            }],
        ),
        (
            "in-finally",
            vec![Stmt::Try {
                body: vec![Stmt::Expr(Expr::Integer(0))],
                catch: None,
                finally: Some(vec![Stmt::Expr(await_(Expr::Integer(1)))]),
            }],
        ),
        (
            "in-if-in-try-in-while",
            vec![Stmt::While {
                condition: Expr::Bool(false),
                body: vec![Stmt::Try {
                    body: vec![Stmt::If {
                        condition: Expr::Bool(true),
                        then_branch: vec![Stmt::Expr(await_(Expr::Integer(1)))],
                        else_branch: None,
                    }],
                    catch: None,
                    finally: None,
                }],
            }],
        ),
        (
            "in-ternary",
            vec![Stmt::Expr(Expr::Conditional {
                condition: Box::new(Expr::Bool(true)),
                then_expr: Box::new(await_(Expr::Integer(1))),
                else_expr: Box::new(Expr::Integer(0)),
            })],
        ),
        (
            "in-logical",
            vec![Stmt::Expr(Expr::Logical {
                op: LogicalOp::And,
                left: Box::new(Expr::Bool(true)),
                right: Box::new(await_(Expr::Integer(1))),
            })],
        ),
        (
            "in-switch-case",
            vec![Stmt::Switch {
                discriminant: Expr::Integer(0),
                cases: vec![SwitchCase {
                    test: Some(Expr::Integer(0)),
                    body: vec![Stmt::Expr(await_(Expr::Integer(1)))],
                }],
            }],
        ),
    ];

    let mut module = Module::new("test");
    let base: perry_hir::types::FuncId = 2000;
    for (i, (_label, body)) in cases.iter().enumerate() {
        let id = base + i as perry_hir::types::FuncId;
        let mut class = empty_class("C");
        class
            .fields
            .push(field_with_init("h", async_closure_body(id, body.clone())));
        module.classes.push(class);
    }

    transform_async_to_generator(&mut module);

    let mut residual: Vec<String> = Vec::new();
    for (i, (label, _body)) in cases.iter().enumerate() {
        if let Some(init) = &module.classes[i].fields[0].init {
            let n = count_raw_awaits_expr(init);
            if n > 0 {
                residual.push(format!("{label}: {n} raw await(s) survived"));
            }
        }
    }
    assert!(
        residual.is_empty(),
        "async-closure async-step rewrite left raw Expr::Await (would \
         block-wait at runtime — the `__85891` crash shape): {residual:#?}"
    );
}

// #8681: async-generator CLOSURE EXPRESSIONS (`const g = async function*(){
// await x; yield y }`) go through `transform_generator_closures_in_stmts`, a
// different path than named async-gen functions. The `-p` crash frame is a
// `perry_closure` — an inline closure — so this path is the closest match.
// After the pipeline no raw `Expr::Await` may survive in the closure or the
// synthesized bodies the transform lifts into `module.functions`.
#[test]
fn async_generator_closure_expressions_linearize_awaits() {
    use crate::generator::transform_generators;

    let async_gen_closure = |id: perry_hir::types::FuncId, body: Vec<Stmt>| Expr::Closure {
        func_id: id,
        params: Vec::new(),
        return_type: Type::Any,
        body,
        captures: Vec::new(),
        mutable_captures: Vec::new(),
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: false,
        is_async: true,
        is_generator: true,
        is_strict: false,
    };
    let y = |v: Expr| {
        Stmt::Expr(Expr::Yield {
            value: Some(Box::new(v)),
            delegate: false,
        })
    };

    let bodies: Vec<(&str, Vec<Stmt>)> = vec![
        ("yield-await", vec![y(await_(Expr::Integer(1)))]),
        (
            "await-in-loop-in-try",
            vec![Stmt::Try {
                body: vec![Stmt::While {
                    condition: Expr::Bool(false),
                    body: vec![Stmt::Expr(await_(Expr::Integer(1))), y(Expr::Integer(2))],
                }],
                catch: None,
                finally: None,
            }],
        ),
        (
            "await-in-if",
            vec![Stmt::If {
                condition: Expr::Bool(true),
                then_branch: vec![Stmt::Expr(await_(Expr::Integer(1))), y(Expr::Integer(2))],
                else_branch: None,
            }],
        ),
    ];

    let mut escaped: Vec<String> = Vec::new();
    for (label, body) in &bodies {
        let mut module = Module::new("test");
        // `const g = async function*(){...}` at module scope.
        module.init.push(Stmt::Let {
            id: 0,
            name: "g".to_string(),
            ty: Type::Any,
            mutable: false,
            init: Some(async_gen_closure(600, body.clone())),
        });
        transform_async_to_generator(&mut module);
        transform_generators(&mut module);
        let residual: usize = module
            .functions
            .iter()
            .map(|f| count_raw_awaits_stmts(&f.body))
            .sum::<usize>()
            + module.init.iter().map(count_raw_awaits_stmt).sum::<usize>();
        if residual > 0 {
            escaped.push(format!("{label}: {residual} raw await(s) survived"));
        }
    }
    assert!(
        escaped.is_empty(),
        "raw Expr::Await survived async-generator CLOSURE linearization \
         (would block-wait at runtime): {escaped:#?}"
    );
}

// A computed-key member body (`[0]() { async () => await 1 }`). The rewrite
// loop already walked `computed_members` (commit f80652ad0) but the collect
// scan did not, so the id set it filters on never listed the closure and the
// walk was dead. With both sides covering computed_members it works.
#[test]
fn async_closure_in_computed_member_body_is_collected() {
    let mut module = Module::new("test");
    let mut class = empty_class("C");
    class.computed_members.push(ClassComputedMember {
        key_expr: Expr::Integer(0),
        function: empty_fn(2, vec![Stmt::Expr(async_closure_with_await(70))]),
        is_static: false,
        kind: ClassComputedMemberKind::Method,
        source_order: 0,
    });
    module.classes.push(class);

    transform_async_to_generator(&mut module);

    assert!(
        module.async_step_closures.contains(&70),
        "computed-member-body async closure FuncId must be collected"
    );
}
