//! An index argument the call site's interval proves (`Proof::Site`) is a
//! proven Number, a canonical double (P3, `facts_rep::canonical_numbers`):
//! it reaches the clone's raw-i32 body through one `fptosi`, with no INT32
//! tag test.

use std::sync::Arc;

use crate::codegen::{ElisionFacts, ProgramFactsView};
use crate::{compile_module, CompileOptions};
use perry_hir::Module;

const SRC: &str = "class Board {\n\
                   cells: number[];\n\
                   constructor() { this.cells = [1, 2, 3, 4, 5, 6, 7, 8]; }\n\
                   at(i: number): number { return this.cells[i] ?? 0; }\n\
                   pick(x: number): number { return this.at(x * 2 + 1); }\n\
                   }\n\
                   export function run(): number { return new Board().pick(2); }\n";

fn lower(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = perry_diagnostics::SourceCache::new();
            let parsed = perry_parser::parse_typescript_with_cache(&src, "ix.ts", &mut cache)
                .expect("parses");
            perry_hir::lower_module(&parsed.module, "ix", "ix.ts").expect("lowers")
        })
        .expect("spawn")
        .join()
        .expect("lowering thread")
}

/// The callee offset of every `.at` call in the HIR.
fn at_offsets(m: &Module) -> Vec<u32> {
    let text = format!("{m:?}");
    let mut out = Vec::new();
    let mut rest = text.as_str();
    let key = "property: \"at\", byte_offset: ";
    while let Some(i) = rest.find(key) {
        rest = &rest[i + key.len()..];
        let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        out.push(num.parse().expect("offset"));
    }
    out
}

/// The IR of `Board.pick`, its `.at` argument proven in `[0, 64]` by the
/// call site's interval.
fn pick_ir() -> String {
    let m = lower(SRC);
    let offsets = at_offsets(&m);
    assert!(!offsets.is_empty(), "{m:?}");
    let elision = ElisionFacts {
        call_arg_ranges: offsets
            .into_iter()
            .map(|o| (o, vec![Some((0, 64))]))
            .collect(),
        ..Default::default()
    };
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
            inside = l.contains("Board__pick");
        }
        if inside {
            out.push(l);
            if l == "}" {
                inside = false;
            }
        }
    }
    assert!(!out.is_empty(), "pick defined:\n{ir}");
    out.join("\n")
}

#[test]
fn a_site_proven_index_argument_converts_with_one_fptosi() {
    let ir = pick_ir();
    assert!(
        ir.contains("$idx_u31") && ir.contains("$i32("),
        "raw clone call:\n{ir}"
    );
    assert!(ir.contains("fptosi double"), "{ir}");
    assert!(
        !ir.contains(crate::nanbox::INT32_TAG_I64),
        "no INT32 tag test on a proven Number:\n{ir}"
    );
}
