//! The packed-numeric clone's counter read is a proven Number, so binding it to
//! a `const` must not shade a GC root — and `arr[i ± c]` must still shade one.
//!
//! `expr_is_known_non_pointer_shadow_value` suppresses
//! `emit_persistent_shadow_root_barrier` for a value that cannot be a heap
//! reference. Inside an active packed-numeric loop fact the entry guard has
//! proved a dense raw-f64 plain Array, the clone has no safepoint and no growth
//! (#9379), and the fast condition bounds the counter by the length read at loop
//! entry — so `arr[i]` reads a raw numeric word.
//!
//! `arr[i + 1]` has none of that: the index can leave the array, and an
//! out-of-bounds element read consults the prototype chain, where
//! `Array.prototype[7] = {}` is a genuine heap reference that must stay rooted.
//! The suppression is therefore restricted to offset 0, and this file is the
//! assertion that the restriction is real.
//!
//! Both reads live in the SAME fast clone, so neither direction can pass
//! vacuously: if the offset read were wrongly admitted the barrier count in the
//! fast body would be 0, and if the counter read were wrongly refused it would
//! be 2. Every test also asserts the clone was entered at all — a barrier count
//! taken over blocks that were never emitted is CLAUDE.md hazard 4.

use perry_hir::types::Type;
use perry_hir::{BinaryOp, CompareOp, Expr, Function, Module as HirModule, Param, Stmt, UpdateOp};

const ARR: u32 = 0;
const SUM: u32 = 1;
const IDX: u32 = 2;
const BOUND_V: u32 = 3;
const BOUND_W: u32 = 4;

/// The root-shading barrier's inline arming test, emitted once per shaded store.
const SHADING_TEST: &str = "@PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT";

fn compile(name: &str, body: Vec<Stmt>) -> String {
    let mut hir = HirModule::new(name);
    hir.functions.push(Function {
        id: 0,
        name: "build".to_string(),
        type_params: Vec::new(),
        params: vec![Param {
            id: ARR,
            name: "a".to_string(),
            ty: Type::Array(Box::new(Type::Number)),
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Number,
        body,
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    let opts = crate::CompileOptions {
        emit_ir_only: true,
        ..Default::default()
    };
    String::from_utf8(crate::compile_module(&hir, opts).expect("test module compiles"))
        .expect("LLVM IR is UTF-8")
}

/// The first emitted block whose label starts with `prefix`, up to the next
/// top-level label. Panics when the block is absent, so an assertion can never
/// be taken over a clone that was not emitted.
fn block(ir: &str, prefix: &str) -> String {
    let start = ir
        .find(&format!("\n{prefix}"))
        .unwrap_or_else(|| panic!("no block labelled {prefix}* was emitted:\n{ir}"));
    let rest = &ir[start + 1..];
    let body_start = rest.find(":\n").expect("a block label ends in a colon") + 2;
    let mut end = rest.len();
    let mut at = body_start;
    for line in rest[body_start..].split_inclusive('\n') {
        let trimmed = line.trim_end();
        if trimmed.ends_with(':') && !trimmed.starts_with(' ') && !trimmed.is_empty() {
            end = at;
            break;
        }
        at += line.len();
    }
    rest[..end].to_string()
}

/// The clone's fast body — where a counter read's binding store lands.
fn fast_body(ir: &str) -> String {
    block(ir, "for.packed_f64_fast.body")
}

/// Where an `arr[i ± c]` binding store lands instead: the offset read carries an
/// inline bounds check that side-exits to the slow preheader, and its store sits
/// past that check rather than in the body block.
fn offset_read_block(ir: &str) -> String {
    block(ir, "packed_f64_loop.foreign.inbounds")
}

/// `let s = 0; for (let i = 0; i < a.length; i++) { <bindings> } return s;`
fn packed_loop_with(bindings: Vec<Stmt>) -> Vec<Stmt> {
    vec![
        Stmt::Let {
            id: SUM,
            name: "s".into(),
            ty: Type::Number,
            init: Some(Expr::Number(0.0)),
            mutable: true,
        },
        Stmt::For {
            init: Some(Box::new(Stmt::Let {
                id: IDX,
                name: "i".into(),
                ty: Type::Number,
                init: Some(Expr::Integer(0)),
                mutable: true,
            })),
            condition: Some(Expr::Compare {
                op: CompareOp::Lt,
                left: Box::new(Expr::LocalGet(IDX)),
                right: Box::new(Expr::PropertyGet {
                    object: Box::new(Expr::LocalGet(ARR)),
                    property: "length".to_string(),
                    byte_offset: 0,
                }),
            }),
            update: Some(Expr::Update {
                id: IDX,
                op: UpdateOp::Increment,
                prefix: false,
            }),
            body: bindings,
        },
        Stmt::Return(Some(Expr::LocalGet(SUM))),
    ]
}

/// `Type::Any`, not `Type::Number`: a number-annotated local is never given a
/// shadow slot, so a barrier could not be emitted for it under any predicate
/// and both arms of the comparison would read 0. The `for…of` desugaring this
/// models erases the element type, which is what earns the slot in the first
/// place — and what makes the suppression worth anything.
fn bind(id: u32, name: &str, index: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: name.into(),
        ty: Type::Any,
        init: Some(Expr::IndexGet {
            object: Box::new(Expr::LocalGet(ARR)),
            index: Box::new(index),
        }),
        mutable: false,
    }
}

fn accumulate(id: u32) -> Stmt {
    Stmt::Expr(Expr::LocalSet(
        SUM,
        Box::new(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::LocalGet(SUM)),
            right: Box::new(Expr::LocalGet(id)),
        }),
    ))
}

fn counter_read() -> Expr {
    Expr::LocalGet(IDX)
}

fn offset_read() -> Expr {
    Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(Expr::LocalGet(IDX)),
        right: Box::new(Expr::Integer(1)),
    }
}

/// `const v = a[i]` alone: the clone's fast body shades nothing.
#[test]
fn a_packed_loop_counter_read_binding_shades_no_root() {
    let ir = compile(
        "packed_counter_read",
        packed_loop_with(vec![
            bind(BOUND_V, "v", counter_read()),
            accumulate(BOUND_V),
        ]),
    );
    let body = fast_body(&ir);
    assert_eq!(
        body.matches(SHADING_TEST).count(),
        0,
        "a[i] under a packed-numeric fact is a proven Number; binding it must shade no root:\n{body}"
    );
}

/// `const w = a[i + 1]` alone: still shaded, because the read can leave the
/// array and reach a prototype index holding a heap reference.
#[test]
fn a_packed_loop_offset_read_binding_still_shades_its_root() {
    let ir = compile(
        "packed_offset_read",
        packed_loop_with(vec![bind(BOUND_W, "w", offset_read()), accumulate(BOUND_W)]),
    );
    let guarded = offset_read_block(&ir);
    assert_eq!(
        guarded.matches(SHADING_TEST).count(),
        1,
        "a[i + 1] can read past the array into the prototype chain; its binding must stay \
         shaded:\n{guarded}"
    );
}

/// Both in one clone, which is what makes neither direction vacuous: the
/// counter read must contribute nothing to the fast body and the offset read
/// must still contribute exactly one to its own guarded block. A non-zero body
/// count would mean the counter read stopped being recognised and the
/// optimisation is dead; a zero guarded count would mean the offset read was
/// wrongly admitted and a prototype-held object could go unshaded.
#[test]
fn only_the_offset_read_shades_when_both_live_in_one_clone() {
    let ir = compile(
        "packed_counter_and_offset",
        packed_loop_with(vec![
            bind(BOUND_V, "v", counter_read()),
            accumulate(BOUND_V),
            bind(BOUND_W, "w", offset_read()),
            accumulate(BOUND_W),
        ]),
    );
    assert_eq!(
        fast_body(&ir).matches(SHADING_TEST).count(),
        0,
        "the counter read's binding must shade nothing:\n{}",
        fast_body(&ir)
    );
    assert_eq!(
        offset_read_block(&ir).matches(SHADING_TEST).count(),
        1,
        "the offset read's binding must still be shaded:\n{}",
        offset_read_block(&ir)
    );
}
