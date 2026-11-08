//! The proven element read (`proven_array.rs`, wpfacts `element_reads`): a
//! read whose receivers the facts prove are fixed-storage program Arrays
//! loads its slot with no receiver, type, forwarding or prefix test, and
//! with no bound when the key is proven inside the length. Without the
//! fact the read keeps the guarded tier, so each test can fail.

use std::sync::Arc;

use crate::codegen::{ElisionFacts, FactsElementRead, ProgramFactsView};
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

fn body<'a>(ir: &'a str, name: &str) -> &'a str {
    let at = ir
        .find(&format!("__{name}("))
        .and_then(|i| ir[..i].rfind("define "))
        .unwrap_or_else(|| panic!("no function {name} in\n{ir}"));
    let end = ir[at..].find("\n}\n").map_or(ir.len(), |e| at + e);
    &ir[at..end]
}

#[test]
fn a_proven_read_loads_its_slot_without_the_guarded_tier() {
    let fact = FactsElementRead {
        len: 128,
        key_in_bounds: false,
    };
    let with = ir(Some(fact));
    for f in ["at", "twice"] {
        let b = body(&with, f);
        assert!(b.contains("parr.load"), "{f}: proven tier\n{b}");
        assert!(!b.contains(".guard.prefix"), "{f}: no guarded tier\n{b}");
        assert!(b.contains("icmp ult i32"), "{f}: the bound stays\n{b}");
        assert!(b.contains(", 128"), "{f}: bound is the length\n{b}");
    }
    let without = ir(None);
    for f in ["at", "twice"] {
        let b = body(&without, f);
        assert!(!b.contains("parr."), "{f}: no fact, no proven tier\n{b}");
        assert!(b.contains(".guard.prefix"), "{f}: guarded tier\n{b}");
    }
}

#[test]
fn a_key_proven_inside_the_length_needs_no_bound() {
    let fact = FactsElementRead {
        len: 128,
        key_in_bounds: true,
    };
    let with = ir(Some(fact));
    let b = body(&with, "at");
    assert!(b.contains("parr.load"), "{b}");
    assert!(!b.contains(", 128"), "no bound against the length\n{b}");
    // A hole still reads through the generic Get.
    assert!(b.contains("perry_array_read_cold_shared"), "{b}");
}
