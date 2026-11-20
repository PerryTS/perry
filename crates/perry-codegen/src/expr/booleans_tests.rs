//! Booleans (`expr::booleans`, wpfacts `booleans`): the truthiness of a
//! local the facts prove a Boolean is one compare with `TAG_TRUE`. Without
//! the fact a bare local keeps the complete test (a declared `boolean`
//! proves nothing), so each test can fail.

use std::sync::Arc;

use crate::codegen::{ElisionFacts, ProgramFactsView};
use crate::{compile_module, CompileOptions};

const SRC: &str = "export function pick(flag: boolean, n: number): number {\n\
                     if (flag) return n + 1;\n\
                     return !flag ? n : 0;\n\
                   }\n";

/// IR of `SRC`; `fact`: its parameter `flag` is a Boolean local.
fn ir(fact: bool) -> String {
    let src = SRC.to_string();
    let m = std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = perry_diagnostics::SourceCache::new();
            let parsed = perry_parser::parse_typescript_with_cache(&src, "bl.ts", &mut cache)
                .expect("parses");
            perry_hir::lower_module(&parsed.module, "bl", "bl.ts").expect("lowers")
        })
        .expect("spawn")
        .join()
        .expect("lowering thread");
    let mut elision = ElisionFacts::default();
    if fact {
        let f = m.functions.iter().find(|f| f.name == "pick").expect("pick");
        elision.boolean_locals.insert(f.params[0].id);
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

/// The complete test's tag arm and its runtime call.
const GENERIC: [&str; 2] = ["truthy.tag", "call i32 @js_is_truthy("];

#[test]
fn an_unproven_boolean_local_keeps_the_complete_test() {
    let without = ir(false);
    for mark in GENERIC {
        assert!(without.contains(mark), "{mark}:\n{without}");
    }
}

#[test]
fn a_proven_boolean_local_is_one_compare() {
    let with = ir(true);
    for mark in GENERIC {
        assert!(!with.contains(mark), "{mark}:\n{with}");
    }
    assert!(
        with.contains("icmp eq i64 %") && with.contains(crate::nanbox::TAG_TRUE_I64),
        "{with}"
    );
}
