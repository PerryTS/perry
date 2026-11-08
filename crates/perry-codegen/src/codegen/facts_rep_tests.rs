//! The facts' `I32` decides a binding's slot (representation RFC §10):
//! a parameter takes the canonical-i32 slot at entry (phase P2 first slice,
//! `facts_rep::bind_params`), and a `let` takes it on the facts' proof
//! alone (P1), with no per-function integer proof. Without the fact both
//! stay doubles.

use std::sync::Arc;

use crate::codegen::{ElisionFacts, FactsRep, ProgramFactsView};
use crate::{compile_module, CompileOptions};
use perry_hir::Module;

const SRC: &str = "export function g(a: number, k: number): number {\n\
                   return (a + k) & 255;\n\
                   }\n\
                   export function h(a: number): number {\n\
                   let r = a;\n\
                   let s = 0;\n\
                   while (r < 8) { s = s + r; r = r + 1; }\n\
                   return s;\n\
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

/// The HIR id of the `let` named `name`.
fn let_id(m: &Module, name: &str) -> u32 {
    let text = format!("{m:?}");
    let mut rest = text.as_str();
    while let Some(i) = rest.find("Let { id: ") {
        rest = &rest[i + 10..];
        let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        let j = rest.find("name: \"").expect("named let");
        if rest[j + 7..].starts_with(&format!("{name}\"")) {
            return num.parse().expect("id");
        }
    }
    panic!("no let {name}");
}

/// The IR of `func`, with `g`'s parameters and `h`'s `r` proven I32 when
/// `proven`.
fn ir_of(func: &str, proven: bool) -> String {
    let m = lower(SRC);
    let mut elision = ElisionFacts::default();
    if proven {
        let mut ids = param_ids(&m);
        ids.push(let_id(&m, "r"));
        for id in ids {
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
        .position(|l| l.starts_with("define") && l.contains(&format!("perry_fn_fp__{func}(")))
        .expect("function defined");
    ir.lines()
        .skip(start)
        .take_while(|l| *l != "}")
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn an_i32_parameter_takes_the_i32_slot() {
    let proven = ir_of("g", true);
    assert!(proven.contains("alloca i32"), "{proven}");
    assert!(proven.contains("add i32"), "{proven}");
    let plain = ir_of("g", false);
    assert!(!plain.contains("add i32"), "{plain}");
}

#[test]
fn a_let_the_facts_give_i32_takes_the_i32_slot() {
    // `r = a` is no integer to the per-function collectors (`a` is any
    // Number): only the facts prove it.
    let proven = ir_of("h", true);
    assert!(proven.contains("add i32"), "{proven}");
    let plain = ir_of("h", false);
    assert!(!plain.contains("add i32"), "{plain}");
}
