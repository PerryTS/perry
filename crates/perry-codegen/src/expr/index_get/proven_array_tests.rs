//! The proven element reads (`proven_array.rs`).
//!
//! The fixed-storage read (wpfacts `element_reads`): a read whose receivers
//! the facts prove are fixed-storage program Arrays loads its slot with no
//! receiver, type, forwarding or prefix test, and with no bound when the
//! key is proven inside the length. Without the fact the read keeps the
//! guarded tier, so each test can fail.
//!
//! The rep-keyed tier (wpfacts `Rep::Arr` of the receiver local, PR-C2):
//! what each field of the rep removes from the read, and that a
//! scalar-replaced literal, which has no header, never takes it.

use std::sync::Arc;

use crate::codegen::{
    ElisionFacts, FactsArrRep, FactsElemRep, FactsElementRead, FactsRep, ProgramFactsView,
};
use crate::{compile_module, CompileOptions};
use perry_hir::Module;

const SRC: &str = "export function at(a: number[], i: number): number { return a[i]; }\n\
                   export function twice(a: number[], i: number): number { return a[i] * 2; }\n";

fn lower(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = perry_diagnostics::SourceCache::new();
            let parsed = perry_parser::parse_typescript_with_cache(&src, "pa.ts", &mut cache)
                .expect("parses");
            perry_hir::lower_module(&parsed.module, "pa", "pa.ts").expect("lowers")
        })
        .expect("spawn")
        .join()
        .expect("lowering thread")
}

/// IR of `SRC` with `fact` on every `a[i]` read.
fn ir(fact: Option<FactsElementRead>) -> String {
    let mut elision = ElisionFacts::default();
    if let Some(f) = fact {
        // SWC offsets are 1-based; a read's offset is its object's start.
        for (i, _) in SRC.match_indices("a[i]") {
            elision.element_reads.insert(i as u32 + 1, f);
        }
    }
    let opts = CompileOptions {
        emit_ir_only: true,
        program_facts: ProgramFactsView {
            elision: Arc::new(elision),
            ..Default::default()
        },
        ..Default::default()
    };
    let m = lower(SRC);
    String::from_utf8(compile_module(&m, opts).expect("fixture compiles")).expect("UTF-8 IR")
}

/// The bound against the length the proven tier emits, as LLVM prints it.
fn bound_128(ir: &str) -> bool {
    ir.lines()
        .any(|l| l.contains("icmp ult i32") && l.trim_end().ends_with(", 128"))
}

#[test]
fn a_proven_read_loads_its_slot_without_the_guarded_tier() {
    let fact = FactsElementRead {
        len: 128,
        key_in_bounds: false,
    };
    let with = ir(Some(fact));
    // Both reads (the value read and the number-context read) take the tier.
    assert!(
        with.matches("parr.load").count() >= 2,
        "proven tier\n{with}"
    );
    assert!(!with.contains(".guard.prefix"), "no guarded tier\n{with}");
    assert!(bound_128(&with), "the bound is the length\n{with}");
    let without = ir(None);
    assert!(
        !without.contains("parr."),
        "no fact, no proven tier\n{without}"
    );
    assert!(without.contains(".guard.prefix"), "guarded tier\n{without}");
}

#[test]
fn a_key_proven_inside_the_length_needs_no_bound() {
    let fact = FactsElementRead {
        len: 128,
        key_in_bounds: true,
    };
    let with = ir(Some(fact));
    assert!(with.contains("parr.load"), "{with}");
    assert!(!bound_128(&with), "no bound against the length\n{with}");
    // A hole still reads through the generic Get.
    assert!(with.contains("perry_array_read_cold_shared"), "{with}");
}

/// A value read and a number-context read under a key the codegen proves
/// an int32 (the mask), so neither needs the runtime key split.
const REP_SRC: &str = "export function at(a: number[], i: number): number { return a[i & 255]; }\n\
                       export function twice(a: number[], i: number): number { return a[i & 255] * 2; }\n";

/// IR of `REP_SRC` with the parameter `a` of both functions `Arr(rep)`;
/// `key`: the key interval of every read; `number`: each read's value is a
/// proven Number.
fn rep_ir(rep: FactsArrRep, key: Option<(i64, i64)>, number: bool) -> String {
    let m = lower(REP_SRC);
    let mut elision = ElisionFacts::default();
    for f in &m.functions {
        elision
            .local_reps
            .insert(f.params[0].id, FactsRep::Arr(rep));
    }
    for (i, _) in REP_SRC.match_indices("a[i & 255]") {
        if let Some(k) = key {
            elision.element_keys.insert(i as u32 + 1, k);
        }
        if number {
            elision.number_element_reads.insert(i as u32 + 1, false);
        }
    }
    let opts = CompileOptions {
        emit_ir_only: true,
        program_facts: ProgramFactsView {
            elision: Arc::new(elision),
            ..Default::default()
        },
        ..Default::default()
    };
    String::from_utf8(compile_module(&m, opts).expect("fixture compiles")).expect("UTF-8 IR")
}

/// The lines of the function whose symbol contains `name`.
fn body<'a>(ir: &'a str, name: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut inside = false;
    for l in ir.lines() {
        if l.starts_with("define") {
            inside = l.contains(name);
        }
        if inside {
            out.push(l);
        }
    }
    assert!(!out.is_empty(), "{name} defined\n{ir}");
    out
}

const BOARD: FactsArrRep = FactsArrRep {
    elem: FactsElemRep::F64,
    floor: 128,
    dense: true,
    fixed: true,
    im: false,
};

/// The forwarding bit of the header word, as the tier tests it.
fn forwarding_test(ir: &str) -> bool {
    ir.lines()
        .any(|l| l.contains("and i32") && l.trim_end().ends_with(", 32768"))
}

#[test]
fn an_in_floor_read_of_a_fixed_dense_array_is_one_load() {
    let ir = rep_ir(BOARD, Some((0, 127)), true);
    for name in ["__at", "__twice"] {
        let f = body(&ir, name).join("\n");
        assert!(!f.contains(".guard.prefix"), "no guarded tier\n{f}");
        assert!(!f.contains("parr."), "no fixed-storage tier\n{f}");
        assert!(!f.contains("arep."), "no test, so no block\n{f}");
        assert!(
            !f.contains("perry_array_read_cold_shared"),
            "no cold arm\n{f}"
        );
        assert!(!bound_128(&f), "no bound\n{f}");
        assert!(!forwarding_test(&f), "fixed: no forwarding test\n{f}");
        // The slot is addressed behind the 24-byte header.
        assert!(
            f.lines()
                .any(|l| l.contains("add i64") && l.trim_end().ends_with(", 24")),
            "front-offset address\n{f}"
        );
    }
}

#[test]
fn a_number_context_keeps_its_number_test_without_the_number_fact() {
    let ir = rep_ir(BOARD, Some((0, 127)), false);
    // The value read needs nothing; the number-context read tests the slot.
    let at = body(&ir, "__at").join("\n");
    assert!(!at.contains("arep.cold"), "{at}");
    // The arithmetic that consumes the load keeps its own number test
    // (P3 B1 removes it with the number fact), so the read adds none.
    let twice = body(&ir, "__twice").join("\n");
    assert!(!twice.contains("arep.cold"), "{twice}");
    assert!(twice.contains("guarded_arith.dynamic"), "{twice}");
    let proven = rep_ir(BOARD, Some((0, 127)), true);
    let twice = body(&proven, "__twice").join("\n");
    assert!(!twice.contains("guarded_arith.dynamic"), "{twice}");
}

#[test]
fn a_key_outside_the_floor_is_bounded_by_the_capacity() {
    // The floor is the least length over the receivers: an index at or
    // past it may still name an element of a longer one, so the bound is
    // the capacity and the slot may be a hole.
    for key in [None, Some((-1, 127)), Some((0, 128))] {
        let ir = rep_ir(BOARD, key, true);
        let f = body(&ir, "__at").join("\n");
        assert!(f.contains("arep.inside"), "{key:?}: a bound\n{f}");
        assert!(!bound_128(&f), "{key:?}: not the floor\n{f}");
        assert!(f.contains("arep.own"), "{key:?}: a hole test\n{f}");
        assert!(f.contains("arep.cold"), "{key:?}: a miss reads cold\n{f}");
        assert!(!forwarding_test(&f), "{key:?}\n{f}");
        assert!(!f.contains(".guard.prefix"), "{key:?}\n{f}");
    }
}

#[test]
fn a_growable_array_keeps_its_forwarding_test() {
    let rep = FactsArrRep {
        fixed: false,
        im: false,
        ..BOARD
    };
    let ir = rep_ir(rep, Some((0, 127)), true);
    let f = body(&ir, "__at").join("\n");
    assert!(forwarding_test(&f), "forwarding test\n{f}");
    assert!(f.contains("arep.live") && f.contains("arep.cold"), "{f}");
    assert!(
        !bound_128(&f) && !f.contains("arep.inside"),
        "no bound\n{f}"
    );
    assert!(!f.contains("arep.own"), "no hole test\n{f}");
}

#[test]
fn a_holey_floor_keeps_its_hole_test() {
    let rep = FactsArrRep {
        dense: false,
        ..BOARD
    };
    let ir = rep_ir(rep, Some((0, 127)), true);
    let f = body(&ir, "__at").join("\n");
    assert!(f.contains("arep.own"), "hole test\n{f}");
    assert!(!bound_128(&f), "no bound\n{f}");
}

#[test]
fn an_array_without_a_floor_is_bounded_by_its_capacity() {
    let rep = FactsArrRep {
        elem: FactsElemRep::F64,
        floor: 0,
        dense: false,
        fixed: false,
        im: false,
    };
    let ir = rep_ir(rep, Some((0, 7)), true);
    let f = body(&ir, "__at").join("\n");
    assert!(forwarding_test(&f), "{f}");
    assert!(f.contains("arep.inside") && f.contains("arep.own"), "{f}");
    assert!(!f.contains(".guard.prefix"), "no guarded tier\n{f}");
    // Its length is not defined, so its front may have moved: the slot is
    // not addressed behind the header.
    assert!(
        !f.lines()
            .any(|l| l.contains("add i64") && l.trim_end().ends_with(", 24")),
        "capacity-relative address\n{f}"
    );
}

#[test]
fn a_scalar_replaced_literal_never_takes_the_tier() {
    // REVIEW-p4l1 R6: a non-escaping literal lives in stack slots and has
    // no heap header, whatever the facts say of its binding.
    let src = "export function pick(i: number): number {\n\
               const d = [5, 4, 3, 2];\n\
               return d[0] + d[3] + i;\n\
               }\n";
    let ir = |fact: bool| {
        let m = lower(src);
        let text = format!("{m:?}");
        let at = text.find("Let { id: ").expect("the literal's let") + 10;
        let id: u32 = text[at..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse()
            .expect("id");
        let mut elision = ElisionFacts::default();
        if fact {
            elision.local_reps.insert(
                id,
                FactsRep::Arr(FactsArrRep {
                    elem: FactsElemRep::F64,
                    floor: 4,
                    dense: true,
                    fixed: true,
                    im: false,
                }),
            );
            for (at, _) in src.match_indices("d[") {
                elision.element_keys.insert(at as u32 + 1, (0, 3));
            }
        }
        let opts = CompileOptions {
            emit_ir_only: true,
            program_facts: ProgramFactsView {
                elision: Arc::new(elision),
                ..Default::default()
            },
            ..Default::default()
        };
        String::from_utf8(compile_module(&m, opts).expect("compiles")).expect("UTF-8 IR")
    };
    let with = ir(true);
    let f = body(&with, "__pick").join("\n");
    assert!(
        !f.contains("js_array_alloc") && !f.contains("perry_array_read_cold_shared"),
        "scalar-replaced\n{f}"
    );
    // The tier would mask the local's word to a pointer and load behind a
    // header; the read stays the scalar slots' own.
    assert_eq!(with, ir(false), "the rep changes nothing\n{f}");
}

// ---- PR-C3: a field read as the receiver, and the in-floor field read call ----

const FIELD_SRC: &str = "export class Board {\n\
  board: number[] = [];\n\
  at(sq: number): number { return this.board[sq] ?? 0; }\n\
  inFloor(): number { return this.at(5); }\n\
  outside(): number { return this.at(200); }\n\
  direct(m: number): number { return this.board[m & 127]; }\n\
}\n";

/// A growable board: dense below 128, storage that may move.
const GROWABLE: FactsArrRep = FactsArrRep {
    elem: FactsElemRep::F64,
    floor: 128,
    dense: true,
    fixed: false,
    im: false,
};

/// IR of `FIELD_SRC` with `this` a `Board` in every method, `board` of
/// representation `rep`, both `this.at(...)` calls field read calls, and
/// `inFloor`'s call in the floor when `in_floor`.
fn field_ir(rep: FactsRep, in_floor: bool) -> String {
    use crate::codegen::{FactsClass, FactsField, FactsFieldReadCall, FactsRecv};
    let m = lower(FIELD_SRC);
    let class = m.classes.iter().find(|c| c.name == "Board").expect("Board");
    let mut elision = ElisionFacts::default();
    for f in &class.methods {
        elision.this_of.insert(
            f.id,
            FactsRecv {
                class_ids: vec![class.id],
                nullable: false,
                checked: false,
            },
        );
    }
    elision.classes.insert(
        class.id,
        FactsClass {
            display: class.name.clone(),
            fixed_layout: true,
            fields: vec![FactsField {
                key: "board".into(),
                slot: 0,
                readonly: true,
                kind_mask: 0x80,
                ta_stable: false,
                ta_kind: None,
                declared: false,
                reserved: false,
                fns: Vec::new(),
                classes: Vec::new(),
                nullable: false,
                rep,
                rest: false,
            }],
        },
    );
    // SWC offsets are 1-based.
    let at = |text: &str| FIELD_SRC.find(text).expect(text) as u32 + 1;
    let read = FactsFieldReadCall {
        field: "board".into(),
        index_arg: 0,
        default: Some(0f64.to_bits()),
        element_offset: at("this.board[sq]"),
        field_offset: at("this.board[sq]"),
        in_floor: None,
        rest: None,
    };
    let inside = at("this.at(5)");
    let outside = at("this.at(200)");
    elision.field_read_calls.insert(
        inside,
        FactsFieldReadCall {
            in_floor: in_floor.then_some(128),
            ..read.clone()
        },
    );
    elision.field_read_calls.insert(outside, read);
    elision.call_arg_ranges.insert(inside, vec![Some((5, 5))]);
    elision
        .call_arg_ranges
        .insert(outside, vec![Some((200, 200))]);
    // The body's own read joins every caller's key; the direct read has
    // its own.
    elision.element_keys.insert(at("this.board[sq]"), (5, 200));
    elision
        .element_keys
        .insert(at("this.board[m & 127]"), (0, 127));
    let opts = CompileOptions {
        emit_ir_only: true,
        program_layouts: Arc::new(crate::layout_families::compute(
            [("pa.ts", &m)],
            true,
            &Default::default(),
        )),
        program_facts: ProgramFactsView {
            elision: Arc::new(elision),
            ..Default::default()
        },
        ..Default::default()
    };
    String::from_utf8(compile_module(&m, opts).expect("fixture compiles")).expect("UTF-8 IR")
}

/// The capacity bound of a read outside the floor.
fn capacity_bound(ir: &str) -> bool {
    ir.contains("arep.inside")
}

#[test]
fn an_in_floor_field_read_call_is_the_field_load_and_the_slot_load() {
    let ir = field_ir(FactsRep::Arr(GROWABLE), true);
    let f = body(&ir, "inFloor").join("\n");
    assert!(!f.contains(".guard.prefix"), "no guarded tier\n{f}");
    assert!(!capacity_bound(&f), "no bound\n{f}");
    assert!(!f.contains("arep.own"), "no hole test\n{f}");
    assert!(!f.contains("arep.number"), "no number test\n{f}");
    // Growable storage: exactly the forwarding test stays, with its cold
    // read.
    assert!(forwarding_test(&f), "{f}");
    assert_eq!(
        f.lines().filter(|l| l.starts_with("arep.live")).count(),
        1,
        "{f}"
    );
    assert!(
        f.lines()
            .any(|l| l.contains("add i64") && l.trim_end().ends_with(", 24")),
        "front-offset address\n{f}"
    );
    // Fixed storage: one load, no test and no cold arm.
    let ir = field_ir(FactsRep::Arr(BOARD), true);
    let f = body(&ir, "inFloor").join("\n");
    assert!(!f.contains("arep."), "{f}");
    assert!(!f.contains("perry_array_read_cold_shared"), "{f}");
    assert!(!forwarding_test(&f), "{f}");
}

#[test]
fn a_field_read_call_outside_the_floor_keeps_its_bound_and_its_default() {
    let ir = field_ir(FactsRep::Arr(GROWABLE), true);
    let f = body(&ir, "outside").join("\n");
    assert!(
        !f.contains(".guard.prefix"),
        "the tier, not the guards\n{f}"
    );
    assert!(capacity_bound(&f), "the capacity bound\n{f}");
    assert!(forwarding_test(&f), "{f}");
    assert!(f.contains("perry_array_read_cold_shared"), "{f}");
    // Without the call's own floor, the in-floor call is the same read.
    let ir = field_ir(FactsRep::Arr(GROWABLE), false);
    let f = body(&ir, "inFloor").join("\n");
    assert!(capacity_bound(&f), "{f}");
}

#[test]
fn a_direct_read_of_an_arr_field_takes_the_tier() {
    let ir = field_ir(FactsRep::Arr(GROWABLE), false);
    let f = body(&ir, "direct").join("\n");
    assert!(!f.contains(".guard.prefix"), "no guarded tier\n{f}");
    assert!(forwarding_test(&f), "{f}");
    assert!(!capacity_bound(&f), "in the floor by its own key\n{f}");
}

#[test]
fn a_boxed_field_keeps_every_read_as_it_was() {
    let boxed = field_ir(FactsRep::Boxed, true);
    assert!(!boxed.contains("arep."), "{boxed}");
    // The call's floor alone changes nothing: the field read is not `Arr`.
    assert_eq!(boxed, field_ir(FactsRep::Boxed, false));
}

#[test]
fn the_array_rep_join_says_what_both_say() {
    let a = FactsArrRep {
        elem: FactsElemRep::I32,
        floor: 128,
        dense: true,
        fixed: true,
        im: false,
    };
    let b = FactsArrRep {
        elem: FactsElemRep::F64,
        floor: 8,
        dense: true,
        fixed: false,
        im: false,
    };
    let j = FactsArrRep::join(a, b);
    assert_eq!(
        (j.elem, j.floor, j.dense, j.fixed),
        (FactsElemRep::F64, 8, true, false)
    );
    assert_eq!(FactsArrRep::join(a, b), FactsArrRep::join(b, a));
    assert_eq!(FactsArrRep::join(a, a), a);
    let holey = FactsArrRep {
        dense: false,
        elem: FactsElemRep::Boxed,
        ..a
    };
    let j = FactsArrRep::join(a, holey);
    assert_eq!((j.elem, j.dense), (FactsElemRep::Boxed, false));
}
