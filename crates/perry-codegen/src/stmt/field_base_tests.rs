//! PR-D (`stmt::field_base`, `expr::index_get::field_base`): rule 1 (a
//! trusted read dominates the back edge) on HIR, and the installed region
//! in the IR on injected facts. Each test has a negative control.

use std::sync::Arc;

use perry_hir::{Expr, Module, Stmt};

use super::field_base::{stmts_read, Paths};
use crate::codegen::{
    ElisionFacts, FactsArrRep, FactsClass, FactsElemRep, FactsField, FactsFieldReadCall, FactsRecv,
    FactsRep, ProgramFactsView,
};
use crate::{compile_module, CompileOptions};

fn lower(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = perry_diagnostics::SourceCache::new();
            let parsed = perry_parser::parse_typescript_with_cache(&src, "fb.ts", &mut cache)
                .expect("parses");
            perry_hir::lower_module(&parsed.module, "fb", "fb.ts").expect("lowers")
        })
        .expect("spawn")
        .join()
        .expect("lowering thread")
}

/// The first loop body in method `name` of class `B`.
fn loop_body<'m>(m: &'m Module, name: &str) -> &'m [Stmt] {
    let class = m.classes.iter().find(|c| c.name == "B").expect("B");
    let f = class
        .methods
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("method {name}"));
    fn find(stmts: &[Stmt]) -> Option<&[Stmt]> {
        stmts.iter().find_map(|s| match s {
            Stmt::While { body, .. } | Stmt::For { body, .. } | Stmt::DoWhile { body, .. } => {
                Some(body.as_slice())
            }
            Stmt::Labeled { body, .. } => find(std::slice::from_ref(body.as_ref())),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => find(then_branch).or_else(|| else_branch.as_deref().and_then(find)),
            Stmt::Try { body, .. } => find(body),
            _ => None,
        })
    }
    find(&f.body).unwrap_or_else(|| panic!("a loop in {name}: {:.600?}", f.body))
}

/// A call `this.at(...)`.
fn is_at(e: &Expr) -> bool {
    matches!(e, Expr::Call { callee, .. }
        if matches!(callee.as_ref(), Expr::PropertyGet { property, .. } if property == "at"))
}

const RULE_SRC: &str = "class B {\n\
  board: number[] = new Array(128).fill(0);\n\
  at(sq: number): number { return this.board[sq] ?? 0; }\n\
  first(n: number): number { let s = 0; let i = 0; while (i < n) { const p = this.at(i); s += p; i++; } return s; }\n\
  test(n: number): number { let s = 0; let i = 0; while (i < n) { if (this.at(i) === 1) s++; i++; } return s; }\n\
  leaves(n: number): number { let s = 0; let i = 0; while (i < n) { if (i > 3) { break; } s += this.at(i); i++; } return s; }\n\
  arms(n: number): number { let s = 0; let i = 0; while (i < n) { if (i > 3) { s += this.at(i); } else { return s; } i++; } return s; }\n\
  skips(n: number): number { let s = 0; for (let i = 0; i < n; i++) { if ((i & 0x88) !== 0) { i += 7; continue; } s += this.at(i); } return s; }\n\
  half(n: number): number { let s = 0; let i = 0; while (i < n) { s += i > 2 ? this.at(i) : 0; i++; } return s; }\n\
  right(n: number): number { let s = 0; let i = 0; while (i < n) { if (i > 2 && this.at(i) === 1) s++; i++; } return s; }\n\
  left(n: number): number { let s = 0; let i = 0; while (i < n) { if (this.at(i) === 1 && i > 2) s++; i++; } return s; }\n\
  inner(n: number): number { let s = 0; for (let r = 0; r < n; r++) { for (let f = 0; f < r; f++) s += this.at(f); } return s; }\n\
  innerTest(n: number): number { let s = 0; for (let r = 0; r < n; r++) { let f = r; while (this.at(f) === 0 && f < 9) f++; s += f; } return s; }\n\
  labeled(n: number): number { let s = 0; outer: for (let r = 0; r < n; r++) { for (let f = 0; f < 3; f++) { if (f === r) continue outer; } s += this.at(r); } return s; }\n\
}\n";

#[test]
fn a_read_that_every_iteration_runs_dominates_the_back_edge() {
    let m = lower(RULE_SRC);
    for name in ["first", "test", "leaves", "arms", "left", "innerTest"] {
        assert_eq!(
            stmts_read(loop_body(&m, name), &is_at),
            Paths::READ,
            "{name}"
        );
    }
}

#[test]
fn a_read_some_iteration_skips_does_not() {
    let m = lower(RULE_SRC);
    // `continue` before the read (evaluate's square loops), a conditional
    // arm, the right operand of `&&`, a nested loop that may run zero
    // times, a labeled jump.
    for name in ["skips", "half", "right", "inner", "labeled"] {
        assert_ne!(
            stmts_read(loop_body(&m, name), &is_at),
            Paths::READ,
            "{name}"
        );
    }
    // With no trusted read nothing dominates.
    assert_ne!(stmts_read(loop_body(&m, "first"), &|_| false), Paths::READ);
}

const IR_SRC: &str = "export class B {\n\
  board: number[] = [];\n\
  at(sq: number): number { return this.board[sq] ?? 0; }\n\
  ray(n: number): number { let s = 0; let i = 0; while (i < 100) { const p = this.at(i & 127); if (p !== 0) break; s += 1; i += 3; } return s; }\n\
  pawn(n: number): number { let s = 0; for (let f = 0; f < 100; f++) { if (this.at(f * 16) === 1) s += 1; } return s; }\n\
  some(n: number): number { let s = 0; for (let i = 0; i < 100; i++) { if (i > 3) s += this.at(i & 127); } return s; }\n\
  sum(n: number): number { let s = 0; for (let i = 0; i < 100; i++) { s += this.at(i & 127) * 2; } return s; }\n\
  grows(n: number): number { let s = 0; for (let i = 0; i < 100; i++) { s += this.at(i & 127); this.board.push(i); } return s; }\n\
}\n";

const GROWABLE: FactsArrRep = FactsArrRep {
    elem: FactsElemRep::F64,
    floor: 128,
    dense: true,
    fixed: false,
    im: false,
};

/// IR of `IR_SRC` with `this` a `B`, `board` of rep `rep`, every
/// `this.at(...)` a field read call, in its floor except `pawn`'s.
fn ir(rep: FactsRep) -> String {
    let m = lower(IR_SRC);
    let class = m.classes.iter().find(|c| c.name == "B").expect("B");
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
            }],
        },
    );
    // SWC offsets are 1-based.
    let at = |text: &str| IR_SRC.find(text).expect(text) as u32 + 1;
    let read = FactsFieldReadCall {
        field: "board".into(),
        index_arg: 0,
        default: Some(0f64.to_bits()),
        element_offset: at("this.board[sq]"),
        field_offset: at("this.board[sq]"),
        in_floor: None,
    };
    for (call, key, floor) in [
        ("this.at(i & 127); if", (0, 127), Some(128)),
        ("this.at(f * 16)", (0, 255), None),
        ("this.at(i & 127); } return", (0, 127), Some(128)),
        ("this.at(i & 127); this.board", (0, 127), Some(128)),
        ("this.at(i & 127) * 2", (0, 127), Some(128)),
    ] {
        let site = at(call);
        elision.field_read_calls.insert(
            site,
            FactsFieldReadCall {
                in_floor: floor,
                ..read.clone()
            },
        );
        elision.call_arg_ranges.insert(site, vec![Some(key)]);
    }
    elision.element_keys.insert(at("this.board[sq]"), (0, 255));
    let opts = CompileOptions {
        emit_ir_only: true,
        program_layouts: Arc::new(crate::layout_families::compute(
            [("fb.ts", &m)],
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

/// The IR of the method whose symbol contains `__B__<name>`.
fn method(ir: &str, name: &str) -> String {
    let tag = format!("__B__{name}");
    let mut out = Vec::new();
    let mut inside = false;
    for line in ir.lines() {
        if line.starts_with("define ") {
            inside = line.contains(&tag);
        }
        if inside {
            out.push(line);
            if line == "}" {
                inside = false;
            }
        }
    }
    out.join("\n")
}

#[test]
fn a_loop_whose_every_iteration_reads_the_board_holds_its_base() {
    let ir = ir(FactsRep::Arr(GROWABLE));
    let f = method(&ir, "ray");
    assert!(f.contains("abase.install"), "guarded install\n{f}");
    // The install resolves forwarding once; the read inside the loop has
    // no forwarding test of its own (C3's `arep.live`).
    assert!(f.contains("abase.follow"), "{f}");
    assert!(!f.contains("arep.live"), "no per-read forwarding test\n{f}");
    // The poll re-derives the base: a second resolution after the safepoint.
    // Arithmetic over the read's Number is no coercion of an object.
    let g = method(&ir, "sum");
    assert!(g.contains("abase.install"), "{g}");
    let after_poll = f
        .split("call void @js_gc_loop_safepoint()")
        .skip(1)
        .any(|tail| tail.contains("js_array_live_head"));
    assert!(after_poll, "refresh after the poll\n{f}");
}

#[test]
fn a_read_outside_its_floor_takes_the_slot_below_the_floor() {
    let ir = ir(FactsRep::Arr(GROWABLE));
    let f = method(&ir, "pawn");
    assert!(f.contains("abase.install"), "{f}");
    assert!(f.contains("abase.slot") && f.contains("abase.read"), "{f}");
    assert!(
        f.lines()
            .any(|l| l.contains("icmp ult i32") && l.trim_end().ends_with(", 128")),
        "the dense floor bounds the slot\n{f}"
    );
}

#[test]
fn a_loop_that_may_skip_the_read_or_grows_the_board_holds_nothing() {
    let ir = ir(FactsRep::Arr(GROWABLE));
    for name in ["some", "grows"] {
        let f = method(&ir, name);
        assert!(!f.contains("abase."), "{name}\n{f}");
    }
    // Without `Arr` no loop holds a base.
    let boxed = ir_boxed();
    assert!(!boxed.contains("abase."), "{boxed}");
}

fn ir_boxed() -> String {
    ir(FactsRep::Boxed)
}
