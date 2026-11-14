//! Literal atoms (`expr::literal_atoms`, wpfacts `literal_strings::atoms`):
//! `x === "lit"` of a local the facts prove holds only pool atoms is the
//! pointer compare alone. Without the fact the compare keeps its other arms
//! (the tag test, the length and the bytes), so each test can fail.

use std::sync::Arc;

use crate::codegen::{ElisionFacts, ProgramFactsView};
use crate::{compile_module, CompileOptions};

const SRC: &str = "export function sign(c: string, n: number): number {\n\
                     if (c === \"white\") return n;\n\
                     return c !== \"black\" ? 0 : -n;\n\
                   }\n";

/// IR of `SRC`; `fact`: its parameter `c` is a literal-atom local.
fn ir(fact: bool) -> String {
    let src = SRC.to_string();
    let m = std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = perry_diagnostics::SourceCache::new();
            let parsed = perry_parser::parse_typescript_with_cache(&src, "la.ts", &mut cache)
                .expect("parses");
            perry_hir::lower_module(&parsed.module, "la", "la.ts").expect("lowers")
        })
        .expect("spawn")
        .join()
        .expect("lowering thread");
    let mut elision = ElisionFacts::default();
    if fact {
        let f = m.functions.iter().find(|f| f.name == "sign").expect("sign");
        elision.literal_atom_locals.insert(f.params[0].id);
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

#[test]
fn an_unproven_operand_keeps_the_complete_compare() {
    let without = ir(false);
    assert!(without.contains("streqlit.tag"), "{without}");
    assert!(without.contains("streqlit.words"), "{without}");
}

#[test]
fn a_literal_atom_operand_is_one_pointer_compare() {
    let with = ir(true);
    assert!(!with.contains("streqlit."), "{with}");
}
