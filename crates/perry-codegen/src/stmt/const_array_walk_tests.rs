//! The for-of walk over an immutable Array (`const_array_walk.rs`): what
//! the facts remove from the loop's entry, the unrolled copies, and the
//! bounds that keep a loop a loop. Without the facts the walk keeps its
//! entry, so each test can fail.

use std::sync::Arc;

use crate::codegen::{ElisionFacts, FactsArrRep, FactsElemRep, FactsRep, ProgramFactsView};
use crate::{compile_module, CompileOptions};
use perry_hir::{Module, Stmt};

const SRC: &str = "export function sum(a: number[], s: number): number {\n\
                     for (const d of a) { if (d === 2) continue; if (d === 9) break; s += d; }\n\
                     return s;\n\
                   }\n";

fn lower(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = perry_diagnostics::SourceCache::new();
            let parsed = perry_parser::parse_typescript_with_cache(&src, "cw.ts", &mut cache)
                .expect("parses");
            perry_hir::lower_module(&parsed.module, "cw", "cw.ts").expect("lowers")
        })
        .expect("spawn")
        .join()
        .expect("lowering thread")
}

/// The walk's source temp of `sum`.
fn source_temp(m: &Module) -> u32 {
    m.functions[0]
        .body
        .iter()
        .find_map(|s| match s {
            Stmt::Let { id, name, .. } if name.starts_with("__forof_lit_src_") => Some(*id),
            _ => None,
        })
        .expect("the walk's source temp")
}

/// IR of `SRC` with the walk's source `Arr` with `im`, of `floor` elements,
/// and constant contents `consts`.
fn ir(im: bool, floor: u32, consts: Option<Vec<f64>>) -> String {
    let m = lower(SRC);
    let t = source_temp(&m);
    let mut elision = ElisionFacts::default();
    elision.local_reps.insert(
        t,
        FactsRep::Arr(FactsArrRep {
            elem: FactsElemRep::F64,
            floor,
            dense: true,
            fixed: true,
            im,
        }),
    );
    if let Some(c) = consts {
        elision
            .const_arrays
            .insert(t, c.iter().map(|v| v.to_bits()).collect());
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

fn blocks(ir: &str, prefix: &str) -> usize {
    ir.lines()
        .filter(|l| l.starts_with(prefix) && l.trim_end().ends_with(':'))
        .count()
}

#[test]
fn without_the_fact_the_walk_keeps_its_entry() {
    let f = ir(false, 3, None);
    assert!(f.contains("forof.walk.header"), "the inline entry\n{f}");
    assert!(f.contains("js_array_record_enter_counted"), "{f}");
    assert_eq!(blocks(&f, "forof.im."), 0, "{f}");
    // The copy into the walk's temp keeps its string demote.
    assert!(blocks(&f, "str_addref.call") >= 1, "{f}");
}

#[test]
fn a_copy_into_an_immutable_array_local_has_no_string_demote() {
    let with = ir(true, 3, Some(vec![1.0, 2.0, 3.0]));
    let without = ir(false, 3, None);
    assert_eq!(
        blocks(&with, "str_addref.call") + 1,
        blocks(&without, "str_addref.call"),
        "one demote test less\n{with}"
    );
}

#[test]
fn constant_contents_unroll_the_walk() {
    let f = ir(true, 3, Some(vec![1.0, 2.0, 3.0]));
    assert!(!f.contains("forof.walk."), "no entry test\n{f}");
    assert!(!f.contains("js_array_record_enter_counted"), "{f}");
    assert!(
        !f.contains("js_array_record_stack_enter"),
        "no record loop\n{f}"
    );
    // Two `next` blocks between three copies, and the exit.
    assert_eq!(blocks(&f, "forof.im.next"), 2, "{f}");
    assert_eq!(blocks(&f, "forof.im.exit"), 1, "{f}");
    // No counted loop and no poll remain.
    assert!(!f.contains("js_gc_loop_safepoint"), "{f}");
    assert_eq!(blocks(&f, "for.cond"), 0, "{f}");
}

#[test]
fn an_empty_constant_array_is_no_loop() {
    let f = ir(true, 0, Some(vec![]));
    assert!(!f.contains("forof.walk."), "{f}");
    assert_eq!(blocks(&f, "forof.im.next"), 0, "{f}");
    assert_eq!(blocks(&f, "forof.im.exit"), 1, "{f}");
}

#[test]
fn more_elements_than_the_bound_keep_the_counted_walk() {
    let f = ir(true, 9, Some((1..=9).map(f64::from).collect()));
    assert!(!f.contains("forof.walk."), "no entry test\n{f}");
    assert!(!f.contains("js_array_record_enter_counted"), "{f}");
    assert_eq!(blocks(&f, "forof.im."), 0, "not unrolled\n{f}");
}

#[test]
fn an_immutable_receiver_without_contents_keeps_the_counted_walk() {
    let f = ir(true, 3, None);
    assert!(!f.contains("forof.walk."), "no entry test\n{f}");
    assert!(!f.contains("js_array_record_enter_counted"), "{f}");
    assert!(
        !f.contains("js_array_record_stack_enter"),
        "no record loop\n{f}"
    );
    assert_eq!(blocks(&f, "forof.im."), 0, "{f}");
}
