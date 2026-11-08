//! A parameter the facts give `I32` (representation RFC §10.4, phase P2
//! first slice, `facts_rep::bind_params`) takes the canonical-i32 slot at
//! entry: the body adds in i32. Without the fact it stays a double.

use std::sync::Arc;

use crate::codegen::{ElisionFacts, FactsRep, ProgramFactsView};
use crate::{compile_module, CompileOptions};
use perry_hir::Module;

const SRC: &str = "export function g(a: number, k: number): number {\n\
                   return (a + k) & 255;\n\
                   }\n";

fn lower(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = perry_diagnostics::SourceCache::new();
            let parsed = perry_parser::parse_typescript_with_cache(&src, "fp.ts", &mut cache)
                .expect("parses");
            perry_hir::lower_module(&parsed.module, "fp", "fp.ts").expect("lowers")
        })
        .expect("spawn")
        .join()
        .expect("lowering thread")
}

/// The HIR ids of `g`'s parameters.
fn param_ids(m: &Module) -> Vec<u32> {
    let g = m
        .functions
        .iter()
        .find(|f| f.name == "g")
        .expect("g lowered");
    g.params.iter().map(|p| p.id).collect()
}

/// The IR of `g`, with its parameters proven I32 when `proven`.
fn ir_of_g(proven: bool) -> String {
    let m = lower(SRC);
    let mut elision = ElisionFacts::default();
    if proven {
        for id in param_ids(&m) {
            elision.number_inputs.insert(id);
            elision.int32_inputs.insert(id);
            elision.local_reps.insert(id, FactsRep::I32);
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
    let ir = String::from_utf8(compile_module(&m, opts).expect("compiles")).expect("UTF-8");
    let start = ir
        .lines()
        .position(|l| l.starts_with("define") && l.contains("perry_fn_fp__g"))
        .expect("g defined");
    ir.lines()
        .skip(start)
        .take_while(|l| *l != "}")
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn an_i32_parameter_takes_the_i32_slot() {
    let proven = ir_of_g(true);
    assert!(proven.contains("alloca i32"), "{proven}");
    assert!(proven.contains("add i32"), "{proven}");
    let plain = ir_of_g(false);
    assert!(!plain.contains("add i32"), "{plain}");
}
