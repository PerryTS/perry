//! A proven int32 local's store (`facts_int32_locals::store_from_double`):
//! the value is a proven Number, a canonical double (representation RFC
//! §10.5, phase P3), so it enters the i32 slot with a bare `fptosi` and no
//! INT32 tag test. Without the fact the store keeps its NaN-safe ToInt32.

use std::sync::Arc;

use crate::codegen::{ElisionFacts, FactsRep, ProgramFactsView};
use crate::{compile_module, CompileOptions};
use perry_hir::Module;

const SRC: &str = "export function f(a: number, b: number): number {\n\
                   let s = a * b;\n\
                   s = s + a;\n\
                   return s & 255;\n\
                   }\n";

fn lower(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = perry_diagnostics::SourceCache::new();
            let parsed = perry_parser::parse_typescript_with_cache(&src, "fi.ts", &mut cache)
                .expect("parses");
            perry_hir::lower_module(&parsed.module, "fi", "fi.ts").expect("lowers")
        })
        .expect("spawn")
        .join()
        .expect("lowering thread")
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

/// The IR of `f`, with `s` proven int32 (and given the I32 representation)
/// when `proven`.
fn ir_of_f(proven: bool) -> String {
    let m = lower(SRC);
    let mut elision = ElisionFacts::default();
    if proven {
        let s = let_id(&m, "s");
        elision.int32_inputs.insert(s);
        elision.local_reps.insert(s, FactsRep::I32);
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
        .position(|l| l.starts_with("define") && l.contains("perry_fn_fi__f"))
        .expect("f defined");
    ir.lines()
        .skip(start)
        .take_while(|l| *l != "}")
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_proven_int32_store_is_a_bare_fptosi() {
    let f = ir_of_f(true);
    assert!(f.contains("fptosi double"), "{f}");
    // The INT32 tag (`INT32_TAG_I64`) is never compared against a stored value.
    let stores_tested = f
        .lines()
        .filter(|l| l.contains(crate::nanbox::INT32_TAG_I64) && l.contains("icmp eq"))
        .count();
    assert_eq!(stores_tested, 0, "{f}");
}
