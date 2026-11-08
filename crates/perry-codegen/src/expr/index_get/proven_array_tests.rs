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
