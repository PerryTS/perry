//! Literal strings (`expr::literal_strings`, wpfacts `literal_strings`): an
//! alias of a local the facts prove holds, as Strings, only literals'
//! values takes no string demote. Without the fact the alias keeps its tag
//! test and the call, so each test can fail.

use std::sync::Arc;

use crate::codegen::{ElisionFacts, ProgramFactsView};
use crate::{compile_module, CompileOptions};

const SRC: &str = "export function pick(by: string, n: number): number {\n\
                     let seen = by;\n\
                     let other = by;\n\
                     if (n > 2) other = seen;\n\
                     return seen.length + other.length + n;\n\
                   }\n";

/// IR of `SRC`; `fact`: its parameter `by` is a literal-string local.
fn ir(fact: bool) -> String {
    let src = SRC.to_string();
    let m = std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = perry_diagnostics::SourceCache::new();
            let parsed = perry_parser::parse_typescript_with_cache(&src, "ls.ts", &mut cache)
                .expect("parses");
            perry_hir::lower_module(&parsed.module, "ls", "ls.ts").expect("lowers")
        })
        .expect("spawn")
        .join()
        .expect("lowering thread");
    let mut elision = ElisionFacts::default();
    if fact {
        let f = m.functions.iter().find(|f| f.name == "pick").expect("pick");
        // `by`, and `seen` (the source of the assignment alias): the next
        // local the body declares.
        let by = f.params[0].id;
        elision.literal_string_locals.insert(by);
        let perry_hir::Stmt::Let { id: seen, .. } = &f.body[0] else {
            panic!("`let seen` first: {:?}", f.body[0]);
        };
        elision.literal_string_locals.insert(*seen);
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

const DEMOTE: &str = "call void @js_string_addref_if_heap_string(";

#[test]
fn an_alias_of_an_unproven_local_keeps_its_demote() {
    let without = ir(false);
    assert!(without.contains(DEMOTE), "{without}");
    assert!(without.contains("str_addref.call"), "{without}");
}

#[test]
fn an_alias_of_a_literal_string_local_takes_no_demote() {
    let with = ir(true);
    assert!(!with.contains(DEMOTE), "{with}");
    assert!(!with.contains("str_addref.call"), "{with}");
}
