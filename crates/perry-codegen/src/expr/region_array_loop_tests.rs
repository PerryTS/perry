//! #10741: loop regions over arrays indexed by the loop COUNTER, with
//! multi-statement bodies, element stores and calls — what the emitted IR
//! must contain.
//!
//! The runtime half (values equal node's when a call reshapes the array mid
//! loop, throws, or the counter/bound is written elsewhere) is
//! `test-files/test_gap_region_counter_loops.ts`. These tests pin what no
//! output can show: that the loop IS admitted (bare `load`/`store double` in
//! F-body, no per-access guard call), that the guard tests the facts the bare
//! accesses rely on, and that a loop whose counter or bound some other
//! statement writes is NOT admitted.

use super::class_field_barrier_tests::ir_opts;
use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{
    BinaryOp, CompareOp, Expr, Function, Module, ModuleInitKind, Param, Stmt, UnaryOp, UpdateOp,
};

const A: u32 = 1;
const B: u32 = 2;
const N: u32 = 3;
const F: u32 = 4;
const I: u32 = 5;

fn opts() -> CompileOptions {
    let mut o = ir_opts();
    o.is_entry_module = false;
    o
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

fn at(arr: u32) -> Expr {
    Expr::IndexGet {
        object: Box::new(Expr::LocalGet(arr)),
        index: Box::new(Expr::LocalGet(I)),
    }
}

/// `arr[i] = value` (the `PutValueSet` form assignment statements lower to).
fn set(arr: u32, value: Expr) -> Stmt {
    Stmt::Expr(Expr::PutValueSet {
        target: Box::new(Expr::LocalGet(arr)),
        key: Box::new(Expr::LocalGet(I)),
        value: Box::new(value),
        receiver: Box::new(Expr::LocalGet(arr)),
        strict: false,
    })
}

fn add(l: Expr, r: Expr) -> Expr {
    Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(l),
        right: Box::new(r),
    }
}

/// `a[i] = a[i] + b[i]; if (a[i] > 100) b[i] = -b[i];` — two statements, a
/// branch, three element reads and two element stores.
fn physics_body() -> Vec<Stmt> {
    vec![
        set(A, add(at(A), at(B))),
        Stmt::If {
            condition: Expr::Compare {
                op: CompareOp::Gt,
                left: Box::new(at(A)),
                right: Box::new(Expr::Number(100.0)),
            },
            then_branch: vec![set(
                B,
                Expr::Unary {
                    op: UnaryOp::Neg,
                    operand: Box::new(at(B)),
                },
            )],
            else_branch: None,
        },
    ]
}

/// `function probe(a, b, n, f) { for (let i = 0; i < n; i++) { body } return 0; }`
/// with `a`/`b` of type `elem`, and `update` replacing `i++` when given.
fn probe_ir(name: &str, elem: Type, body: Vec<Stmt>, update: Option<Expr>) -> String {
    let mut m = Module::new(name);
    m.functions = vec![Function {
        id: 1,
        name: "probe".to_string(),
        type_params: Vec::new(),
        params: vec![
            param(A, "a", elem.clone()),
            param(B, "b", elem),
            param(N, "n", Type::Number),
            param(F, "f", Type::Any),
        ],
        return_type: Type::Number,
        body: vec![
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: I,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(I)),
                    right: Box::new(Expr::LocalGet(N)),
                }),
                update: Some(update.unwrap_or(Expr::Update {
                    id: I,
                    op: UpdateOp::Increment,
                    prefix: false,
                })),
                body,
            },
            Stmt::Return(Some(Expr::Integer(0))),
        ],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }];
    m.init_kind = ModuleInitKind::Eager;
    String::from_utf8(compile_module(&m, opts()).expect("module compiles")).expect("UTF-8 IR")
}

fn number_array() -> Type {
    Type::Array(Box::new(Type::Number))
}

/// The probe function's text, split into (label, lines) blocks.
fn probe_blocks(ir: &str) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    let mut in_fn = false;
    for line in ir.lines() {
        if line.starts_with("define ") && line.contains("probe") {
            in_fn = true;
            out.push(("entry".to_string(), Vec::new()));
            continue;
        }
        if !in_fn {
            continue;
        }
        if line.starts_with('}') {
            in_fn = false;
            continue;
        }
        if !line.starts_with(' ') && line.ends_with(':') {
            out.push((line.trim_end_matches(':').to_string(), Vec::new()));
        } else if let Some(b) = out.last_mut().filter(|_| !line.trim().is_empty()) {
            b.1.push(line.trim().to_string());
        }
    }
    out
}

/// The IR of each function compiled from `probe` (its specialised and
/// generic clones and the entry), one string each: block labels repeat
/// across clones, so a count over [`f_body`] of the whole module counts
/// every clone that formed the region.
fn probe_fn_irs(ir: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur: Option<String> = None;
    for line in ir.lines() {
        if line.starts_with("define ") && line.contains("probe") {
            cur = Some(String::new());
        }
        if let Some(c) = cur.as_mut() {
            c.push_str(line);
            c.push('\n');
            if line.starts_with('}') {
                out.extend(cur.take());
            }
        }
    }
    out
}

/// Lines of the blocks reachable from an F-body entry (`rloop.fast*`)
/// without leaving through the split's join.
fn f_body(ir: &str) -> Vec<String> {
    let blocks = probe_blocks(ir);
    let succ = |lines: &[String]| -> Vec<String> {
        lines
            .last()
            .map(|t| {
                t.split("label %")
                    .skip(1)
                    .map(|x| {
                        x.chars()
                            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.')
                            .collect()
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut seen: Vec<String> = Vec::new();
    let mut work: Vec<String> = blocks
        .iter()
        .filter(|(l, _)| l.starts_with("rloop.fast"))
        .map(|(l, _)| l.clone())
        .collect();
    while let Some(l) = work.pop() {
        if l.starts_with("rloop.join") || seen.contains(&l) {
            continue;
        }
        if let Some((_, lines)) = blocks.iter().find(|(b, _)| *b == l) {
            work.extend(succ(lines));
        }
        seen.push(l);
    }
    let mut out = Vec::new();
    for (label, lines) in &blocks {
        if seen.contains(label) {
            out.push(format!("{label}:"));
            out.extend(lines.iter().cloned());
        }
    }
    out
}

#[test]
fn a_multi_statement_counter_loop_is_admitted_with_bare_element_accesses() {
    let ir = probe_ir("rarr_physics", number_array(), physics_body(), None);
    let f = f_body(&ir);
    assert!(!f.is_empty(), "the loop formed no region:\n{ir}");
    let f_text = f.join("\n");
    assert!(
        f_text.contains("store double"),
        "F-body must store the element as a raw double:\n{f_text}"
    );
    assert!(
        !f_text.contains("index_get_guard") && !f_text.contains("index_set_guard"),
        "F-body must not guard an element access:\n{f_text}"
    );
    assert!(
        !f_text.contains("@js_dynamic_neg(")
            && !f_text.contains("@js_rel_gt(")
            && !f_text.contains("@js_dynamic_string_or_number_add("),
        "a bare element read is a Number: no dynamic operator in F-body:\n{f_text}"
    );
    // The facts the bare accesses rely on, in the preheader guard: the dense
    // raw-f64 layout bit, the store word (integrity bits), the bound against
    // the length.
    assert!(
        ir.contains(", 8388608"),
        "the guard must test GC_ARRAY_RAW_F64_LAYOUT:\n{ir}"
    );
    assert!(
        ir.contains(", 67600639"),
        "a storing region must test the integrity bits:\n{ir}"
    );
    assert!(
        ir.contains("fcmp ole double"),
        "the guard must test the counter's bound against the length:\n{ir}"
    );
}

#[test]
fn a_float64array_receiver_reads_canonicalise_nan() {
    let ir = probe_ir(
        "rarr_typed",
        Type::Named("Float64Array".to_string()),
        physics_body(),
        None,
    );
    let f_text = f_body(&ir).join("\n");
    assert!(!f_text.is_empty(), "the loop formed no region:\n{ir}");
    assert!(
        ir.contains("rloop.ta"),
        "a receiver not declared a plain Array must also admit a Float64Array:\n{ir}"
    );
    assert!(
        f_text.contains("fcmp ord double"),
        "a Float64Array read may hold any NaN payload: F must canonicalise it:\n{f_text}"
    );
    let plain = probe_ir("rarr_plain", number_array(), physics_body(), None);
    assert!(
        !plain.contains("rloop.ta"),
        "a receiver declared a plain Array admits only a dense array:\n{plain}"
    );
}

#[test]
fn a_typed_region_guard_names_only_symbols_that_exist() {
    // #10516 removed the process-wide `PERRY_TA_VIEW_GUARD` in favour of the
    // typed array header's own storage byte. The typed region guard, merged
    // after it (#10741), still loaded the removed global, and the text
    // assertions above cannot see a reference to a symbol nothing declares.
    // Any receiver not declared a plain Array (or a typed array without raw
    // f64 slots) takes this guard, so ask LLVM, the one check that sees an
    // undefined value.
    for (name, elem) in [
        ("rarr_typed_verify", Type::Named("Float64Array".to_string())),
        ("rarr_untyped_verify", Type::Any),
    ] {
        let ir = probe_ir(name, elem, physics_body(), None);
        assert!(
            ir.contains("rloop.ta"),
            "{name}: the loop must take the typed region guard for this test to mean anything:\n{ir}"
        );
        assert!(
            !ir.contains("@PERRY_TA_VIEW_GUARD"),
            "{name}: the guard reads the receiver's storage byte, not a removed global:\n{ir}"
        );
        crate::testing::verify_ir(&ir, name)
            .unwrap_or_else(|e| panic!("{name}: LLVM rejected the module: {e}\n{ir}"));
    }
}

#[test]
fn a_typed_array_without_f64_slots_forms_no_region() {
    // `perm[i] = perm1[i]` over two `Int32Array`s (fannkuch's copy loop):
    // neither the dense guard nor the Float64Array guard can pass for an
    // integer typed array, so a region there is a guard that fails on every
    // entry plus a second copy of the loop. With `n` an untyped bound, the
    // counter-bound scope would otherwise admit it. A Float64Array, and a
    // receiver with no declared type, still form one.
    let copy = || vec![set(A, at(B))];
    for kind in ["Int32Array", "Uint8Array", "Float32Array", "Buffer"] {
        let ir = probe_ir("rarr_int_copy", Type::Named(kind.to_string()), copy(), None);
        assert!(
            !ir.contains("rloop.fast"),
            "{kind}: no region guard can pass for it, so none may form:\n{ir}"
        );
        crate::testing::verify_ir(&ir, "rarr_int_copy")
            .unwrap_or_else(|e| panic!("{kind}: LLVM rejected the module: {e}\n{ir}"));
    }
    for elem in [Type::Named("Float64Array".to_string()), Type::Any] {
        let ir = probe_ir("rarr_f64_copy", elem.clone(), copy(), None);
        assert!(
            ir.contains("rloop.fast") && ir.contains("rloop.ta"),
            "{elem:?}: a Float64Array is served by the typed guard:\n{ir}"
        );
    }
}

#[test]
fn a_call_in_the_body_sets_the_dirty_flag_and_the_next_iteration_rechecks() {
    let mut body = physics_body();
    body.insert(
        1,
        Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::LocalGet(F)),
            args: vec![Expr::LocalGet(A)],
            type_args: Vec::new(),
            byte_offset: 0,
        }),
    );
    let ir = probe_ir("rarr_call", number_array(), body, None);
    let f = f_body(&ir);
    assert!(
        !f.is_empty(),
        "a call must end the facts, not refuse the loop:\n{ir}"
    );
    assert!(
        ir.contains("rloop.recheck"),
        "the iteration after the call must re-check the facts:\n{ir}"
    );
    // After the call, F-body stores `true` into the dirty flag, and an
    // element access after it is not bare (the guarded tier is back).
    let f_text = f.join("\n");
    assert!(
        f.iter().any(|l| l.starts_with("store i1 true")),
        "F-body must set the dirty flag after the call:\n{f_text}"
    );
    assert!(
        f.iter()
            .any(|l| l.contains("index_get_guard") || l.contains("@js_rel_gt(")),
        "an element access after the call must not be bare:\n{f_text}"
    );
    assert!(
        f.iter().any(|l| l.starts_with("store double")),
        "the accesses before the call stay bare:\n{f_text}"
    );
}

/// C1's shape for the counter: every write of the counter and of its bound
/// must be accounted for, in the body AND the condition AND the update.
#[test]
fn a_counter_or_bound_written_outside_the_update_is_not_a_region_index() {
    // `i` also written in the body.
    let mut body = physics_body();
    body.push(Stmt::Expr(Expr::Update {
        id: I,
        op: UpdateOp::Increment,
        prefix: false,
    }));
    let ir = probe_ir("rarr_counter_body", number_array(), body, None);
    assert!(
        !ir.contains("rloop.fast"),
        "a counter written in the body is not a region index:\n{ir}"
    );
    // The bound written in the update (`i++, n = n + 1`).
    let update = Expr::Sequence(vec![
        Expr::Update {
            id: I,
            op: UpdateOp::Increment,
            prefix: false,
        },
        Expr::LocalSet(N, Box::new(add(Expr::LocalGet(N), Expr::Integer(1)))),
    ]);
    let ir = probe_ir(
        "rarr_bound_update",
        number_array(),
        physics_body(),
        Some(update),
    );
    assert!(
        !ir.contains("rloop.fast"),
        "a bound written in the update is not a region bound:\n{ir}"
    );
    // The bound written in the body.
    let mut body = physics_body();
    body.push(Stmt::Expr(Expr::LocalSet(
        N,
        Box::new(add(Expr::LocalGet(N), Expr::Integer(1))),
    )));
    let ir = probe_ir("rarr_bound_body", number_array(), body, None);
    assert!(
        !ir.contains("rloop.fast"),
        "a bound written in the body is not a region bound:\n{ir}"
    );
}

#[test]
fn a_store_of_a_value_not_proven_a_number_is_not_bare() {
    // `b[i] = b[i] + 1; a[i] = "s";` — the first store is bare, the second
    // must be today's store (it clears the array's raw-f64 layout).
    let body = vec![
        set(B, add(at(B), Expr::Integer(1))),
        set(A, Expr::String("s".to_string())),
    ];
    let ir = probe_ir("rarr_string_store", number_array(), body, None);
    // Each clone of `probe` (specialised, generic) that forms the region has
    // its own F-body; each is checked on its own.
    let fs: Vec<Vec<String>> = probe_fn_irs(&ir)
        .iter()
        .map(|f| f_body(f))
        .filter(|f| !f.is_empty())
        .collect();
    assert!(!fs.is_empty(), "the loop formed no region:\n{ir}");
    for f in &fs {
        let f_text = f.join("\n");
        // Bare stores are emitted in the F-body's own blocks; today's store
        // lowers inside its `idxset.*` blocks.
        let mut label = "";
        let mut bare_stores = 0;
        for l in f {
            if l.ends_with(':') {
                label = l;
            } else if l.starts_with("store double") && !label.starts_with("idxset") {
                bare_stores += 1;
            }
        }
        assert_eq!(
            bare_stores, 1,
            "only `b[i] = b[i] + 1` may be a bare raw store:\n{f_text}"
        );
        assert!(
            f.iter()
                .any(|l| l.contains("index_set") && l.contains("call ")),
            "the string store must take today's store:\n{f_text}"
        );
    }
}

/// `a[i & 63]`, a static index in `[0, 63]`.
fn masked(arr: u32) -> Expr {
    Expr::IndexGet {
        object: Box::new(Expr::LocalGet(arr)),
        index: Box::new(Expr::Binary {
            op: BinaryOp::BitAnd,
            left: Box::new(Expr::LocalGet(I)),
            right: Box::new(Expr::Integer(63)),
        }),
    }
}

/// An array the loop only reads element VALUES from (`const o = a[i & 63];
/// f(o)`) in a body that calls out is no region array: the region would save
/// one load per read and re-check its guard on every iteration. A read a
/// Number consumer takes makes it one, and so does the value read in a body
/// that runs no JS (`const o = a[i & 63]; o.d = i`), where the facts hold
/// across iterations.
#[test]
fn an_array_only_read_for_element_values_is_not_a_region_array() {
    let call = |arg: Expr| {
        Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::LocalGet(F)),
            args: vec![arg],
            type_args: Vec::new(),
            byte_offset: 0,
        })
    };
    const O: u32 = 10;
    let body = vec![
        Stmt::Let {
            id: O,
            name: "o".to_string(),
            ty: Type::Any,
            mutable: false,
            init: Some(masked(A)),
        },
        call(Expr::LocalGet(O)),
    ];
    let ir = probe_ir(
        "rarr_value_only",
        Type::Array(Box::new(Type::Any)),
        body,
        None,
    );
    assert!(
        !ir.contains("rloop.arr."),
        "a value-only read must not make its array a region array:\n{ir}"
    );
    // Control: the same read consumed by a Number operator does.
    let body = vec![call(add(masked(A), Expr::Number(1.0)))];
    let ir = probe_ir("rarr_value_numeric", number_array(), body, None);
    assert!(
        ir.contains("rloop.arr."),
        "a Number-consumed read keeps its array a region array:\n{ir}"
    );
    // Control: a value read in a body without a call does too.
    let body = vec![
        Stmt::Let {
            id: O,
            name: "o".to_string(),
            ty: Type::Any,
            mutable: false,
            init: Some(masked(A)),
        },
        Stmt::Expr(Expr::PropertySet {
            object: Box::new(Expr::LocalGet(O)),
            property: "d".to_string(),
            value: Box::new(Expr::LocalGet(I)),
        }),
    ];
    let ir = probe_ir(
        "rarr_value_no_call",
        Type::Array(Box::new(Type::Any)),
        body,
        None,
    );
    assert!(
        ir.contains("rloop.arr."),
        "a value read in a call-free body keeps its array a region array:\n{ir}"
    );
}
