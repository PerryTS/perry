//! Step 4b loop regions (`stmt::region_loop`): what the emitted IR must contain.
//!
//! Two of the region's obligations are invisible to every runtime probe, so
//! the IR is the only evidence (the argument is `write_pic_barrier_tests.rs`'s
//! header, and #8183's record that a release build with a store barrier
//! deleted passes every runtime matrix byte-identically):
//!
//! 1. a bare store of a pointer-capable value owes the GC the store IC's
//!    bookkeeping (remembered set + incremental-mark shading + layout note);
//! 2. a region that stores tests the two per-object store facts that are not
//!    shape facts yet (DESIGN §6.5a F-A, F-B) in its guard.
//!
//! Each test is written to fail under the matching sabotage of
//! `region_loop.rs` (recorded in `/root/linktime/PROGRESS.md`).

use super::class_field_barrier_tests::ir_opts;
use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{
    BinaryOp, CompareOp, Expr, Function, Module, ModuleInitKind, Param, Stmt, UnaryOp, UpdateOp,
};
use std::collections::{HashMap, HashSet};

const O: u32 = 1;
const V: u32 = 2;
const N: u32 = 3;
const I: u32 = 4;
const H: u32 = 5;

fn opts() -> CompileOptions {
    let mut o = ir_opts();
    o.is_entry_module = false;
    o
}

fn param(id: u32, name: &str) -> Param {
    Param {
        id,
        name: name.to_string(),
        ty: Type::Any,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

fn get(key: &str) -> Expr {
    Expr::PropertyGet {
        object: Box::new(Expr::LocalGet(O)),
        property: key.to_string(),
        byte_offset: 0,
    }
}

fn put(key: &str, value: Expr) -> Stmt {
    Stmt::Expr(Expr::PutValueSet {
        target: Box::new(Expr::LocalGet(O)),
        key: Box::new(Expr::String(key.to_string())),
        value: Box::new(value),
        receiver: Box::new(Expr::LocalGet(O)),
        strict: false,
    })
}

/// `function probe(o, v, n) { let h = 0; for (let i = 0; i < n; i++) { body } return h; }`
fn loop_ir_with_return(name: &str, body: Vec<Stmt>, result: Expr) -> String {
    loop_ir_with_bound(name, body, result, Expr::LocalGet(N))
}

fn loop_ir_with_bound(name: &str, body: Vec<Stmt>, result: Expr, bound: Expr) -> String {
    let mut m = Module::new(name);
    m.functions = vec![Function {
        id: 1,
        name: "probe".to_string(),
        type_params: Vec::new(),
        params: vec![param(O, "o"), param(V, "v"), param(N, "n")],
        return_type: Type::Any,
        body: vec![
            Stmt::Let {
                id: H,
                name: "h".to_string(),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Number(0.0)),
            },
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
                    right: Box::new(bound),
                }),
                update: Some(Expr::Update {
                    id: I,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body,
            },
            Stmt::Return(Some(result)),
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

fn loop_ir(name: &str, body: Vec<Stmt>) -> String {
    loop_ir_with_return(name, body, Expr::LocalGet(H))
}

/// The blocks of the probe function, label -> (instructions, successors).
fn blocks(ir: &str) -> HashMap<String, (Vec<String>, Vec<String>)> {
    let mut out = HashMap::new();
    let mut in_fn = false;
    let mut cur: Option<String> = None;
    let mut insts: Vec<String> = Vec::new();
    let flush = |cur: &mut Option<String>,
                 insts: &mut Vec<String>,
                 out: &mut HashMap<String, (Vec<String>, Vec<String>)>| {
        if let Some(l) = cur.take() {
            let succ: Vec<String> = insts
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
                .unwrap_or_default();
            out.insert(l, (std::mem::take(insts), succ));
        }
    };
    for line in ir.lines() {
        if line.starts_with("define ") && line.contains("probe") {
            in_fn = true;
            continue;
        }
        if !in_fn {
            continue;
        }
        if line.starts_with('}') {
            flush(&mut cur, &mut insts, &mut out);
            in_fn = false;
            continue;
        }
        if !line.starts_with(' ') && line.ends_with(':') {
            flush(&mut cur, &mut insts, &mut out);
            cur = Some(line.trim_end_matches(':').to_string());
            continue;
        }
        if !line.trim().is_empty() {
            insts.push(line.trim().to_string());
        }
    }
    out
}

/// Every block reachable from an F-body entry (`rloop.fast*`) without
/// leaving through the split's join.
fn f_body_blocks(bl: &HashMap<String, (Vec<String>, Vec<String>)>) -> HashSet<String> {
    let mut seen = HashSet::new();
    let mut work: Vec<String> = bl
        .keys()
        .filter(|l| l.starts_with("rloop.fast"))
        .cloned()
        .collect();
    while let Some(l) = work.pop() {
        if l.starts_with("rloop.join") || !seen.insert(l.clone()) {
            continue;
        }
        if let Some((_, succ)) = bl.get(&l) {
            work.extend(succ.iter().cloned());
        }
    }
    seen
}

#[test]
fn a_bare_pointer_store_keeps_the_store_ics_gc_bookkeeping() {
    // `o.x = v; o.y = i;` — `v` is `any`: pointer-capable.
    let ir = loop_ir(
        "region_loop_barrier",
        vec![put("x", Expr::LocalGet(V)), put("y", Expr::LocalGet(I))],
    );
    let bl = blocks(&ir);
    let f = f_body_blocks(&bl);
    assert!(
        !f.is_empty(),
        "the loop must form a region: no `rloop.fast` block in\n{ir}"
    );
    let barrier_in_f = f.iter().any(|l| {
        bl[l]
            .0
            .iter()
            .any(|i| i.contains("call void @js_write_barrier_slot_validated_parent("))
    });
    assert!(
        barrier_in_f,
        "a bare store of an `any` value in F-body must carry the remembered-set \
         / incremental-mark barrier; F-body blocks: {f:?}"
    );
}

#[test]
fn a_storing_region_guards_exact_shape_and_classless_receiver() {
    let ir = loop_ir(
        "region_loop_admission",
        vec![
            put("y", Expr::LocalGet(I)),
            Stmt::Expr(Expr::LocalSet(
                H,
                Box::new(Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(Expr::LocalGet(H)),
                    right: Box::new(get("x")),
                }),
            )),
        ],
    );
    let bl = blocks(&ir);
    let chk: Vec<&String> = bl
        .keys()
        .filter(|l| l.starts_with("rloop.guard.chk"))
        .collect();
    assert!(!chk.is_empty(), "no region guard in\n{ir}");
    for l in chk {
        let body = bl[l].0.join("\n");
        assert!(
            body.contains("load i32") && body.contains("icmp eq i32"),
            "proof-bearing ShapeId guard missing from {l}:\n{body}"
        );
        assert!(
            body.contains(", 768"),
            "F-A (class-less receiver kind, `_reserved & 0x300`) missing from {l}:\n{body}"
        );
        assert!(
            !body.contains(", 128"),
            "retired numeric-proof header guard reappeared in {l}:\n{body}"
        );
    }
}

#[test]
fn a_read_only_region_guard_does_not_test_store_facts() {
    let ir = loop_ir(
        "region_loop_readonly",
        vec![Stmt::Expr(Expr::LocalSet(
            H,
            Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(get("x")),
                right: Box::new(get("y")),
            }),
        ))],
    );
    let bl = blocks(&ir);
    let chk: Vec<&String> = bl
        .keys()
        .filter(|l| l.starts_with("rloop.guard.chk"))
        .collect();
    assert!(!chk.is_empty(), "no region guard in\n{ir}");
    for l in chk {
        assert!(
            !bl[l].0.join("\n").contains(", 768"),
            "a region that never stores must not pay for the store admission"
        );
    }
}

#[test]
fn a_read_region_carries_a_spill_copy_whose_reads_go_through_the_spill_buffer() {
    // `h = h + o.x` — `x` is only read, so its word may name a spill slot.
    let ir = loop_ir(
        "region_loop_spill",
        vec![Stmt::Expr(Expr::LocalSet(
            H,
            Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::LocalGet(H)),
                right: Box::new(get("x")),
            }),
        ))],
    );
    let bl = blocks(&ir);
    assert!(
        bl.keys().any(|l| l.starts_with("rloop.version.spill")),
        "a region reading a key it never stores must select a spill copy on a flipped word:\n{ir}"
    );
    let spill_reads: Vec<&String> = bl
        .keys()
        .filter(|l| l.starts_with("rloop.slot.spill"))
        .collect();
    assert!(!spill_reads.is_empty(), "no spill-addressed read in\n{ir}");
    for l in spill_reads {
        let body = bl[l].0.join("\n");
        // meta, then ObjectMeta.spill (+32), then the element past the
        // 8-byte array header at (field - 32).
        assert!(
            body.matches("load i64").count() >= 2
                && body.contains(", 32")
                && body.contains("sub i64"),
            "{l} must load ObjectHeader.meta and ObjectMeta.spill and index by field - 32:\n{body}"
        );
    }
    // The guard recognises the spill word by the flipped id (S5 convention).
    let flips: Vec<&String> = bl
        .keys()
        .filter(|l| l.starts_with("rloop.guard.flip"))
        .collect();
    assert!(!flips.is_empty(), "no flip compare in\n{ir}");
    assert!(
        flips
            .iter()
            .all(|l| bl[*l].0.join("\n").contains("xor i32")),
        "the spill word is recognised by its ShapeId with PACKED_SPILL_FLIP flipped"
    );
}

#[test]
fn a_region_that_stores_every_key_it_names_has_no_spill_copy() {
    let ir = loop_ir("region_loop_nospill", vec![put("y", Expr::LocalGet(I))]);
    let bl = blocks(&ir);
    assert!(
        bl.keys().any(|l| l.starts_with("rloop.guard.chk")),
        "the loop must still form a region:\n{ir}"
    );
    assert!(
        !bl.keys()
            .any(|l| l.starts_with("rloop.version.spill") || l.starts_with("rloop.slot.spill")),
        "a stored key is published only inline, so no spill copy is needed:\n{ir}"
    );
}

/// The prime call's penultimate argument: the boxed-store mask (charter step 5).
fn prime_boxed_masks(ir: &str) -> Vec<u32> {
    ir.lines()
        .filter(|l| l.contains("@js_region_loop_prime("))
        .filter_map(|l| {
            // The R mask follows the boxed-store mask; the call may carry
            // trailing LLVM attributes after its closing parenthesis.
            let (before_r, _) = l.rsplit_once(", i32 ")?;
            before_r.rsplit_once("i32 ")?.1.trim().parse().ok()
        })
        .collect()
}

/// Charter step 5: a bare store runs no field-representation check, so the
/// region tells the runtime which keys it may store a value not proven a
/// canonical double into; the runtime refuses a word whose shape has a
/// non-`Any` lane at such a key (`region_loop_pack`, `F64Stored`).
#[test]
fn a_bare_store_of_a_value_not_proven_a_double_names_its_key_to_the_prime() {
    let boxed = loop_ir("region_loop_boxed", vec![put("x", Expr::LocalGet(V))]);
    let masks = prime_boxed_masks(&boxed);
    assert!(!masks.is_empty(), "no prime call in\n{boxed}");
    assert!(
        masks.iter().all(|&m| m == 1),
        "the `any` store to `x` must name key 0: {masks:?}"
    );
    let raw = loop_ir(
        "region_loop_raw",
        vec![
            put("x", Expr::Number(1.5)),
            Stmt::Expr(Expr::LocalSet(
                H,
                Box::new(Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(Expr::LocalGet(H)),
                    right: Box::new(get("x")),
                }),
            )),
        ],
    );
    let masks = prime_boxed_masks(&raw);
    assert!(!masks.is_empty(), "no prime call in\n{raw}");
    assert!(
        masks.iter().all(|&m| m == 0),
        "a literal double is a valid value of every lane: {masks:?}"
    );
}

/// Last prime argument, before LLVM call attributes, is the requested region R mask.
fn prime_rep_masks(ir: &str) -> Vec<u32> {
    ir.lines()
        .filter(|line| line.contains("@js_region_loop_prime("))
        .filter_map(|line| {
            line.rsplit_once("i32 ")?
                .1
                .split_once(')')?
                .0
                .trim()
                .parse()
                .ok()
        })
        .collect()
}

/// P8: the generic region must carry the class-field increment shape after
/// its older numeric loop tier is retired. The store is bare only when its
/// own exact read is protected by R; a string store cannot clear the boxed
/// mask even when a later read requests R.
#[test]
fn a_region_r_proven_increment_store_clears_only_its_number_boxed_bit() {
    let increment = Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(get("x")),
        right: Box::new(Expr::Integer(1)),
    };
    let numeric = loop_ir("region_store_r_number", vec![put("x", increment)]);
    let numeric_masks = prime_boxed_masks(&numeric);
    assert!(
        numeric.contains("rloop.fast"),
        "numeric region did not form:\n{numeric}"
    );
    assert!(
        !numeric_masks.is_empty() && numeric_masks.iter().all(|&m| m == 0),
        "R-proven Number store must clear the boxed bit: {numeric_masks:?}\n{numeric}"
    );
    let numeric_r = prime_rep_masks(&numeric);
    assert!(
        !numeric_r.is_empty() && numeric_r.iter().all(|&m| m == 1),
        "increment must actually request F64 for its exact read: {numeric_r:?}\n{numeric}"
    );

    let non_number = loop_ir(
        "region_store_r_string",
        vec![
            put("x", Expr::String("bad".into())),
            Stmt::Expr(Expr::LocalSet(
                H,
                Box::new(Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(get("x")),
                    right: Box::new(Expr::Integer(1)),
                }),
            )),
        ],
    );
    let non_number_masks = prime_boxed_masks(&non_number);
    assert!(
        !non_number_masks.is_empty() && non_number_masks.iter().all(|&m| m == 1),
        "string store must retain the boxed bit: {non_number_masks:?}\n{non_number}"
    );
}

/// A fresh bare read used by a Number-consuming add requests R. The prime
/// serves an Any lane only with `REGION_LOOP_WORD_VALUE_TEST`, which the
/// guard honours with a value test, so this is an actual R-bearing region
/// rather than a vacuous mask argument.
#[test]
fn a_number_consuming_bare_read_sets_the_prime_rep_mask() {
    let ir = loop_ir(
        "region_loop_rep",
        vec![Stmt::Expr(Expr::LocalSet(
            H,
            Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::LocalGet(H)),
                right: Box::new(get("x")),
            }),
        ))],
    );
    assert!(ir.contains("rloop.fast"), "region did not form:\n{ir}");
    let masks = prime_rep_masks(&ir);
    assert!(!masks.is_empty(), "no learned prime in\n{ir}");
    assert!(
        masks.iter().all(|&m| m == 1),
        "fresh x read must request key 0: {masks:?}"
    );
}

/// The R-bearing guard tests the R slot's value whenever the learned word
/// carries the value-test bit (bit 63: a signed compare against 0), and the
/// test is the strict Number test below the tag band.
#[test]
fn a_number_read_guard_value_tests_a_word_that_asks() {
    let ir = loop_ir(
        "region_loop_value_test",
        vec![Stmt::Expr(Expr::LocalSet(
            H,
            Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::LocalGet(H)),
                right: Box::new(get("x")),
            }),
        ))],
    );
    let bl = blocks(&ir);
    let value: Vec<&Vec<String>> = bl
        .iter()
        .filter(|(l, _)| {
            l.strip_prefix("rloop.guard.value.")
                .is_some_and(|t| t.chars().all(|c| c.is_ascii_digit()))
        })
        .map(|(_, (insts, _))| insts)
        .collect();
    assert!(!value.is_empty(), "no value-test block in\n{ir}");
    assert!(
        value.iter().all(|insts| {
            insts.iter().any(|i| i.contains("load double"))
                && insts
                    .iter()
                    .any(|i| i.contains("and i64") && i.contains("9223372036854775807"))
                && insts
                    .iter()
                    .any(|i| i.contains("icmp ult i64") && i.contains("9221401712017801216"))
        }),
        "every value test must load the slot and test it below the tag band:\n{value:#?}"
    );
    assert!(
        bl.iter()
            .filter(|(l, _)| l.starts_with("rloop.guard.value.need"))
            .all(|(_, (insts, _))| insts.iter().any(|i| i.contains("icmp slt i64"))),
        "the value test must be selected by the word's sign bit"
    );
}

/// A read beneath another property access is that access's receiver
/// (`o.x.length`), never a Number operand: the region asks for no R.
#[test]
fn a_receiver_read_beneath_a_property_access_requests_no_number_lane() {
    let ir = loop_ir(
        "region_loop_receiver_not_operand",
        vec![Stmt::Expr(Expr::LocalSet(
            H,
            Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::LocalGet(H)),
                right: Box::new(Expr::PropertyGet {
                    object: Box::new(get("x")),
                    property: "length".to_string(),
                    byte_offset: 0,
                }),
            }),
        ))],
    );
    let masks = prime_rep_masks(&ir);
    assert!(!masks.is_empty(), "the region must form:\n{ir}");
    assert!(
        masks.iter().all(|&m| m == 0),
        "`o.x` is `.length`'s receiver, not a Number operand: {masks:?}"
    );
    assert!(!ir.contains("rloop.guard.value"), "no R, so no value test");
}

/// A value only tested for truthiness is not a Number operand: neither a
/// conditional's test (`h += o.x ? 1 : 0`; the arms are the values) nor a `!`
/// operand asks the region for R, directly or through the accumulator's
/// value flow. A string `x` would otherwise fail the value test on every
/// iteration and the loop would pay the guard for nothing.
#[test]
fn a_truthiness_test_requests_no_number_lane() {
    let ternary = |test: Expr| Expr::Conditional {
        condition: Box::new(test),
        then_expr: Box::new(Expr::Integer(1)),
        else_expr: Box::new(Expr::Integer(0)),
    };
    for (name, test) in [
        ("region_loop_truthy_cond", get("x")),
        (
            "region_loop_truthy_not",
            Expr::Unary {
                op: UnaryOp::Not,
                operand: Box::new(get("x")),
            },
        ),
    ] {
        let ir = loop_ir(
            name,
            vec![Stmt::Expr(Expr::LocalSet(
                H,
                Box::new(Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(Expr::LocalGet(H)),
                    right: Box::new(ternary(test)),
                }),
            ))],
        );
        let masks = prime_rep_masks(&ir);
        assert!(
            masks.iter().all(|&m| m == 0),
            "{name}: a truthiness test is not a Number operand: {masks:?}\n{ir}"
        );
        assert!(
            !ir.contains("rloop.guard.value"),
            "{name}: no R, so no value test"
        );
    }
    // Control: the same read as an arm IS the value, and asks for R.
    let ir = loop_ir(
        "region_loop_truthy_arm",
        vec![Stmt::Expr(Expr::LocalSet(
            H,
            Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::LocalGet(H)),
                right: Box::new(Expr::Conditional {
                    condition: Box::new(Expr::LocalGet(N)),
                    then_expr: Box::new(get("x")),
                    else_expr: Box::new(Expr::Integer(0)),
                }),
            }),
        ))],
    );
    let masks = prime_rep_masks(&ir);
    assert!(
        !masks.is_empty() && masks.iter().all(|&m| m == 1),
        "an arm is a Number operand: {masks:?}\n{ir}"
    );
}

/// The F-local fixed point follows the fresh F64 read through a temporary and
/// a loop-carried accumulator. Entry is strict; G and post-loop code retain
/// the ordinary dynamic add. Removing the scoped materialization or the
/// entry check makes this test fail.
#[test]
fn a_region_number_local_is_admitted_only_in_f() {
    const TEMP: u32 = 6;
    let body = vec![
        Stmt::Let {
            id: TEMP,
            name: "temp".to_string(),
            ty: Type::Any,
            mutable: false,
            init: Some(get("x")),
        },
        Stmt::Expr(Expr::LocalSet(
            H,
            Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::LocalGet(H)),
                right: Box::new(Expr::LocalGet(TEMP)),
            }),
        )),
    ];
    let ir = loop_ir_with_return(
        "region_number_scope",
        body,
        Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::LocalGet(H)),
            right: Box::new(Expr::Integer(1)),
        },
    );
    assert!(ir.contains("rloop.fast"), "region did not form:\n{ir}");
    let masks = prime_rep_masks(&ir);
    assert!(
        !masks.is_empty() && masks.iter().all(|&m| m == 1),
        "the temp's source must request R=1: {masks:?}\n{ir}"
    );
    assert!(
        ir.contains("rloop.fast") && ir.contains("fadd double"),
        "F must use numeric add:\n{ir}"
    );
    assert!(
        ir.contains("rloop.guard") && ir.contains("icmp ult i64"),
        "A_F must strictly test the loop-carried accumulator:\n{ir}"
    );
    assert!(
        ir.lines()
            .filter(|line| {
                line.contains("call ") && line.contains("@js_dynamic_string_or_number_add(")
            })
            .count()
            >= 2,
        "G and post-loop adds must remain dynamic (no scope leak):\n{ir}"
    );
}

/// Literal-bound flow through temp must receive the same R/5L proof as direct reads.
#[test]
fn flow_derived_number_local_constant_bound_avoids_recheck() {
    const TEMP: u32 = 6;
    let body = vec![
        Stmt::Let {
            id: TEMP,
            name: "temp".to_string(),
            ty: Type::Any,
            mutable: false,
            init: Some(get("x")),
        },
        Stmt::Expr(Expr::LocalSet(
            H,
            Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::LocalGet(H)),
                right: Box::new(Expr::LocalGet(TEMP)),
            }),
        )),
    ];
    let ir = loop_ir_with_bound(
        "region_number_scope_constant_bound",
        body,
        Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::LocalGet(H)),
            right: Box::new(Expr::Integer(1)),
        },
        Expr::Integer(200),
    );
    assert!(ir.contains("rloop.fast"), "region did not form:\n{ir}");
    assert!(
        !ir.contains("br i1 true, label %rloop.recheck"),
        "constant-bound flow-derived R/5L must avoid an unconditional recheck:\n{ir}"
    );
    let bl = blocks(&ir);
    let f = f_body_blocks(&bl);
    assert!(!f.is_empty(), "F body must be present:\n{ir}");
    for label in f {
        let instructions = bl[&label].0.join("\n");
        assert!(
            !instructions.contains("@js_object_get_field"),
            "{label} must retain the bare R-proven load:\n{instructions}\n{ir}"
        );
    }
    let masks = prime_rep_masks(&ir);
    assert!(
        !masks.is_empty() && masks.iter().all(|&m| m == 1),
        "the temp's source must request R=1: {masks:?}\n{ir}"
    );
    assert!(
        ir.contains("rloop.fast") && ir.contains("fadd double"),
        "F must use numeric add:\n{ir}"
    );
    assert!(
        ir.contains("rloop.guard") && ir.contains("icmp ult i64"),
        "A_F must strictly test the loop-carried accumulator:\n{ir}"
    );
    assert!(
        ir.lines()
            .filter(|line| {
                line.contains("call ") && line.contains("@js_dynamic_string_or_number_add(")
            })
            .count()
            >= 2,
        "G and post-loop adds must remain dynamic (no scope leak):\n{ir}"
    );
}

/// An unrestricted bound comparison may invoke user code and revoke freshness.
#[test]
fn any_bound_numeric_region_retains_collecting_comparison_recheck() {
    const TEMP: u32 = 6;
    let body = vec![
        Stmt::Let {
            id: TEMP,
            name: "temp".to_string(),
            ty: Type::Any,
            mutable: false,
            init: Some(get("x")),
        },
        Stmt::Expr(Expr::LocalSet(
            H,
            Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::LocalGet(H)),
                right: Box::new(Expr::LocalGet(TEMP)),
            }),
        )),
    ];
    let ir = loop_ir_with_return(
        "region_number_scope_any_bound",
        body,
        Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::LocalGet(H)),
            right: Box::new(Expr::Integer(1)),
        },
    );
    assert!(ir.contains("rloop.fast"), "region did not form:\n{ir}");
    assert!(
        ir.contains("@js_rel_lt("),
        "Any bound must retain its collecting comparison:\n{ir}"
    );
    assert!(
        ir.contains("br i1 true, label %rloop.recheck"),
        "Any-bound user-code comparison must retain the conservative recheck:\n{ir}"
    );
    let masks = prime_rep_masks(&ir);
    assert!(
        !masks.is_empty() && masks.iter().all(|&m| m == 1),
        "the temp's source must request R=1: {masks:?}\n{ir}"
    );
    assert!(
        ir.contains("rloop.fast") && ir.contains("fadd double"),
        "F must use numeric add:\n{ir}"
    );
    assert!(
        ir.contains("rloop.guard") && ir.contains("icmp ult i64"),
        "A_F must strictly test the loop-carried accumulator:\n{ir}"
    );
    assert!(
        ir.lines()
            .filter(|line| {
                line.contains("call ") && line.contains("@js_dynamic_string_or_number_add(")
            })
            .count()
            >= 2,
        "G and post-loop adds must remain dynamic (no scope leak):\n{ir}"
    );
}

fn times(e: Expr, factor: i64) -> Expr {
    Expr::Binary {
        op: BinaryOp::Mul,
        left: Box::new(e),
        right: Box::new(Expr::Integer(factor)),
    }
}

fn accumulated(reads: &[&str], factor: i64) -> Stmt {
    let mut sum = Expr::LocalGet(H);
    for key in reads {
        sum = Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(sum),
            right: Box::new(times(get(key), factor)),
        };
    }
    Stmt::Expr(Expr::LocalSet(H, Box::new(sum)))
}

/// An arithmetic wrapper must consume the exact R and 5L facts, regardless
/// of its spelling. Sabotaging either proof restores the unconditional
/// recheck and (for four reads) generic reads after the first one.
#[test]
fn numeric_arithmetic_twins_keep_all_reads_bare_without_a_recheck() {
    for keys in [&["a"][..], &["a", "b", "c", "e"][..]] {
        for factor in [1, 2] {
            let ir = loop_ir_with_bound(
                "region_arith_twin",
                vec![accumulated(keys, factor)],
                Expr::LocalGet(H),
                Expr::Integer(200),
            );
            let masks = prime_rep_masks(&ir);
            let expected = (1u32 << keys.len()) - 1;
            assert!(
                !masks.is_empty() && masks.iter().all(|m| *m == expected),
                "every exact arithmetic read must be R-proven: {masks:?}\n{ir}"
            );
            assert!(ir.contains("rloop.fast"), "F did not form:\n{ir}");
            assert!(
                !ir.contains("rloop.recheck"),
                "numeric arithmetic must not recheck every iteration:\n{ir}"
            );
            let bl = blocks(&ir);
            let f = f_body_blocks(&bl);
            for label in f {
                let instructions = bl[&label].0.join("\n");
                assert!(
                    !instructions.contains("@js_object_get_field"),
                    "{label} must not use a generic read:\n{instructions}\n{ir}"
                );
            }
        }
    }
}

/// A property read from another object may run a getter. It must kill the
/// receiver fact even when it is nested under native arithmetic, and a
/// later read of the region receiver must not inherit the earlier proof.
#[test]
fn arithmetic_getter_operand_requires_generic_recheck() {
    let getter = Expr::PropertyGet {
        object: Box::new(Expr::Call {
            callee: Box::new(Expr::LocalGet(V)),
            args: Vec::new(),
            type_args: Vec::new(),
            byte_offset: 0,
        }),
        property: "x".to_string(),
        byte_offset: 0,
    };
    let body = vec![
        Stmt::Expr(Expr::LocalSet(
            H,
            Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::LocalGet(H)),
                right: Box::new(Expr::Binary {
                    op: BinaryOp::Mul,
                    left: Box::new(get("a")),
                    right: Box::new(getter),
                }),
            }),
        )),
        Stmt::Expr(get("b")),
    ];
    let ir = loop_ir_with_bound(
        "region_getter_kill",
        body,
        Expr::LocalGet(H),
        Expr::Integer(200),
    );
    assert!(
        ir.contains("rloop.fast"),
        "fixture must admit the first read:\n{ir}"
    );
    assert!(
        ir.contains("rloop.recheck") && ir.contains("br i1 true, label %rloop.recheck"),
        "getter operand must force the back-edge recheck:\n{ir}"
    );
    assert!(
        prime_rep_masks(&ir).iter().all(|m| m & 0b10 == 0),
        "the later b read must not borrow the stale fact:\n{ir}"
    );
}

/// A call after the final bare read may mutate the receiver or its
/// prototype. The whole F path, not just prefixes of bare reads, is part of
/// the next iteration's freshness proof.
#[test]
fn post_read_mutation_call_forces_next_iteration_recheck() {
    let body = vec![
        accumulated(&["a"], 1),
        Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::LocalGet(V)),
            args: Vec::new(),
            type_args: Vec::new(),
            byte_offset: 0,
        }),
    ];
    let ir = loop_ir_with_bound(
        "region_post_read_mutation",
        body,
        Expr::LocalGet(H),
        Expr::Integer(200),
    );
    assert!(
        ir.contains("rloop.fast"),
        "fixture must admit the read:\n{ir}"
    );
    assert!(
        ir.contains("rloop.recheck") && ir.contains("br i1 true, label %rloop.recheck"),
        "a post-read JS call must recheck before the next iteration:\n{ir}"
    );
}
