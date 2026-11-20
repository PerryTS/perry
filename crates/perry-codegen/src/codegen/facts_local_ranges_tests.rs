//! The whole-program interval of a proven int32 `let` is a range proof
//! (`expr::range_facts::facts_local_range`): a key built from the local is
//! an int32 by the interval, so it is computed in i32 and handed to the
//! element read as it is. Without the interval the key is a double chain
//! and goes through the index normalizer.

use std::sync::Arc;

use crate::codegen::{ElisionFacts, FactsRep, ProgramFactsView};
use crate::{compile_module, CompileOptions};
use perry_hir::Module;

/// `k` has no local range proof (an element of an ordinary Array): only
/// the facts' interval bounds it.
const SRC: &str = "export function f(t: number[], u: number[], i: number): number {\n\
                   const k = u[i];\n\
                   return t[k * 2 + 1] ?? -1;\n\
                   }\n";

fn lower(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = perry_diagnostics::SourceCache::new();
            let parsed = perry_parser::parse_typescript_with_cache(&src, "lr.ts", &mut cache)
                .expect("parses");
            perry_hir::lower_module(&parsed.module, "lr", "lr.ts").expect("lowers")
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

/// The IR of `f` with `k` proven int32, and its interval `range` when given.
fn ir_of_f(range: Option<(i64, i64)>) -> String {
    let m = lower(SRC);
    let mut elision = ElisionFacts::default();
    let k = let_id(&m, "k");
    elision.int32_inputs.insert(k);
    elision.local_reps.insert(k, FactsRep::I32);
    if let Some(range) = range {
        elision.int32_ranges.insert(k, range);
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
        .position(|l| l.starts_with("define") && l.contains("perry_fn_lr__f"))
        .expect("f defined");
    ir.lines()
        .skip(start)
        .take_while(|l| *l != "}")
        .collect::<Vec<_>>()
        .join("\n")
}

fn normalizer_calls(ir: &str) -> usize {
    ir.lines()
        .filter(|l| l.contains("call") && l.contains("perry_array_index_normalize_i32"))
        .count()
}

/// The normalizer calls of `f` when `k` has no interval: one for `t[k * 2 +
/// 1]` (and one for `u[i]`, whose key is a double parameter in every arm).
fn baseline() -> usize {
    let n = normalizer_calls(&ir_of_f(None));
    assert!(
        n >= 1,
        "the key of `t[k * 2 + 1]` is normalized without an interval"
    );
    n
}

#[test]
fn a_key_built_from_a_ranged_int32_let_is_an_i32_key() {
    // `k` in [-3, 12]: `k * 2 + 1` is in [-5, 25], an int32 of either sign:
    // its normalizer call is gone, and the key is an i32 chain.
    let f = ir_of_f(Some((-3, 12)));
    assert_eq!(normalizer_calls(&f) + 1, baseline(), "{f}");
    assert!(f.contains("mul i32") || f.contains("shl i32"), "{f}");
}

#[test]
fn an_interval_whose_chain_leaves_int32_is_no_i32_key() {
    // `k` up to 2^31 - 1: `k * 2 + 1` may pass int32, so the key keeps its
    // double (a wrapped i32 would name another element).
    let f = ir_of_f(Some((0, i64::from(i32::MAX))));
    assert_eq!(normalizer_calls(&f), baseline(), "{f}");
}
