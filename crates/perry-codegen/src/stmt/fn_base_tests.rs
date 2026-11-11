//! N2 (`stmt::fn_base`): the held suffix of a method body in the IR, on
//! injected facts. Each positive test has a negative control.

use std::sync::Arc;

use perry_hir::Module;

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
            let parsed = perry_parser::parse_typescript_with_cache(&src, "fnb.ts", &mut cache)
                .expect("parses");
            perry_hir::lower_module(&parsed.module, "fnb", "fnb.ts").expect("lowers")
        })
        .expect("spawn")
        .join()
        .expect("lowering thread")
}

const SRC: &str = "export class B {\n\
  board: number[] = [];\n\
  at(sq: number): number { return this.board[sq] ?? 0; }\n\
  three(n: number): number { return this.at(3) + this.at(5) + this.at(7); }\n\
  one(n: number): number { return this.at(9); }\n\
  calls(n: number): number { let s = this.at(1); this.board.push(1); s += this.at(2); return s; }\n\
  after(n: number): number { this.board.push(2); return this.at(4) + this.at(6); }\n\
  outer(n: number): number { let s = 0; if (this.at(8) === 1) s = 5; let i = 0; while (i < 100) { const p = this.at(i & 127); if (p !== 0) break; s += 1; i += 3; } return s; }\n\
}\n";

const GROWABLE: FactsArrRep = FactsArrRep {
    elem: FactsElemRep::F64,
    floor: 128,
    dense: true,
    fixed: false,
    im: false,
    at_rest: false,
};

/// IR of `SRC` with `this` a `B`, `board` of rep `rep`, every `this.at(...)`
/// an in-floor field read call.
fn ir(rep: FactsRep) -> String {
    let m = lower(SRC);
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
    let at = |text: &str| SRC.find(text).expect(text) as u32 + 1;
    let read = FactsFieldReadCall {
        field: "board".into(),
        index_arg: 0,
        default: Some(0f64.to_bits()),
        element_offset: at("this.board[sq]"),
        field_offset: at("this.board[sq]"),
        in_floor: Some(128),
        rest: None,
    };
    let mut offset = 0;
    while let Some(k) = SRC[offset..].find("this.at(") {
        let site = (offset + k) as u32 + 1;
        elision.field_read_calls.insert(site, read.clone());
        elision.call_arg_ranges.insert(site, vec![Some((0, 127))]);
        offset += k + 1;
    }
    elision.element_keys.insert(at("this.board[sq]"), (0, 255));
    let opts = CompileOptions {
        emit_ir_only: true,
        program_layouts: Arc::new(crate::layout_families::compute(
            [("fnb.ts", &m)],
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
            inside = line.contains(&tag) && !line.contains(&format!("{tag}_"));
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
fn straight_line_reads_share_one_install() {
    let text = ir(FactsRep::Arr(GROWABLE));
    let f = method(&text, "three");
    // One install (one forwarding resolution) serves the three reads, none
    // of which tests forwarding itself (C3's `arep.live`).
    assert_eq!(f.matches("call i64 @js_array_live_head").count(), 1, "{f}");
    assert!(!f.contains("arep.live"), "{f}");
    // Negative control: a single read installs nothing.
    let g = method(&text, "one");
    assert!(!g.contains("abase."), "{g}");
}

#[test]
fn a_call_ends_the_region_and_the_suffix_starts_after_it() {
    let text = ir(FactsRep::Arr(GROWABLE));
    // A read before the push and one after: the suffix holds one site.
    let f = method(&text, "calls");
    assert!(!f.contains("abase."), "{f}");
    // Two reads after the push: installed after it.
    let g = method(&text, "after");
    let (before, tail) = g.split_once("js_array_push").expect("the push");
    assert!(!before.contains("abase."), "{g}");
    assert!(tail.contains("abase.follow"), "{g}");
}

#[test]
fn a_loop_in_the_suffix_is_served_and_installs_nothing() {
    let text = ir(FactsRep::Arr(GROWABLE));
    let f = method(&text, "outer");
    assert!(f.contains("abase.follow"), "{f}");
    assert!(!f.contains("abase.install"), "no loop install\n{f}");
    // The poll re-derives the function-level base.
    let after_poll = f
        .split("call void @js_gc_loop_safepoint()")
        .skip(1)
        .any(|tail| tail.contains("js_array_live_head"));
    assert!(after_poll, "{f}");
    // Without `Arr` nothing is held.
    let boxed = ir(FactsRep::Boxed);
    assert!(!boxed.contains("abase."), "{boxed}");
}
