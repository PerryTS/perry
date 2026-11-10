//! N1 temporaries (`temps`): selections of i32 operands lower as i32, a
//! test of two exact int32 operands is one `icmp`, and a selection with a
//! non-integer operand stays double.

use std::sync::Arc;

use crate::codegen::{ElisionFacts, FactsRep, ProgramFactsView};
use crate::{compile_module, CompileOptions};
use perry_hir::Module;

const SRC: &str = "export function k(n: number): number {\n\
                   let t = 0;\n\
                   for (let s = 0; s < n; s++) {\n\
                   const file = s & 7;\n\
                   for (let f = Math.max(0, file - 1); f <= Math.min(7, file + 1); f++) t = t + f;\n\
                   const c = (s & 2) !== 0 ? file + 1 : file;\n\
                   const d = Math.min(file, 2.5);\n\
                   t = t + c + d;\n\
                   }\n\
                   return t;\n\
                   }\n";

fn lower(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = perry_diagnostics::SourceCache::new();
            let parsed = perry_parser::parse_typescript_with_cache(&src, "tp.ts", &mut cache)
                .expect("parses");
            perry_hir::lower_module(&parsed.module, "tp", "tp.ts").expect("lowers")
        })
        .expect("spawn")
        .join()
        .expect("lowering thread")
}

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

/// The IR of `k`'s bodies, with `s`, `file`, `f` and `c` I32 by the facts.
fn ir_of_k() -> String {
    let m = lower(SRC);
    let mut elision = ElisionFacts::default();
    for name in ["s", "file", "f", "c"] {
        let id = let_id(&m, name);
        elision.number_inputs.insert(id);
        elision.int32_inputs.insert(id);
        elision.local_reps.insert(id, FactsRep::I32);
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
    let mut out = Vec::new();
    let mut inside = false;
    for l in ir.lines() {
        if l.starts_with("define") {
            inside = l.contains("@perry_fn_tp__k");
        }
        if inside {
            out.push(l);
            if l == "}" {
                inside = false;
            }
        }
    }
    assert!(!out.is_empty(), "k defined");
    out.join("\n")
}

#[test]
fn integer_selections_and_their_tests_stay_i32() {
    if !super::enabled() {
        return;
    }
    let ir = ir_of_k();
    // `Math.max(0, file - 1)` and `Math.min(7, file + 1)`.
    assert!(ir.contains("@llvm.smax.i32"), "{ir}");
    assert!(ir.contains("@llvm.smin.i32"), "{ir}");
    // `f <= Math.min(7, file + 1)` is one integer compare.
    assert!(ir.contains("icmp sle i32"), "{ir}");
    // The conditional of two integer arms is an i32 phi.
    assert!(ir.contains("phi i32"), "{ir}");
    // Negative control: `Math.min(file, 2.5)` is no integer selection, so
    // the only `smin` is the loop bound's, whose first operand is `7`.
    for l in ir.lines().filter(|l| l.contains("@llvm.smin.i32")) {
        assert!(l.contains("@llvm.smin.i32(i32 7,"), "{l}\n{ir}");
    }
}
