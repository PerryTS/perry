//! Return-type inference for long `&&` / `||` chains (#11510). Minified
//! predicates such as a codepoint-range test are one flat logical chain with
//! hundreds of comparisons; the recursive rule spent one inference-depth level
//! per join and collapsed them to `Any` past `INFER_TYPE_RECURSION_CAP`.

#![cfg(test)]

use crate::lower_module;
use crate::types::Type;
use crate::Module;
use perry_diagnostics::SourceCache;
use perry_parser::parse_typescript_with_cache;

fn lower_src(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = parse_typescript_with_cache(&src, "test.ts", &mut cache)
                .expect("parse should succeed");
            lower_module(&parsed.module, "test", "test.ts").expect("lowering should succeed")
        })
        .expect("spawn")
        .join()
        .expect("lowering thread")
}

fn return_type(module: &Module, func: &str) -> Type {
    module
        .functions
        .iter()
        .find(|f| f.name == func)
        .unwrap_or_else(|| panic!("function {func} not lowered"))
        .return_type
        .clone()
}

/// A YF6-shaped predicate: `ranges` `q>=a&&q<=b` pairs interleaved with
/// `singles` `q===c` tests, all joined by `||`.
fn range_predicate(ranges: usize, singles: usize) -> String {
    let mut terms = Vec::new();
    let mut k = 4352;
    for i in 0..ranges.max(singles) {
        if i < ranges {
            terms.push(format!("q>={k}&&q<={}", k + 7));
            k += 16;
        }
        if i < singles {
            terms.push(format!("q==={k}"));
            k += 3;
        }
    }
    format!("function YF6(q){{return {}}}\n", terms.join("||"))
}

#[test]
fn short_logical_chain_is_boolean() {
    let module = lower_src(&range_predicate(2, 2));
    assert_eq!(return_type(&module, "YF6"), Type::Boolean);
}

#[test]
fn deep_logical_chain_keeps_boolean_return_type() {
    // 76 ranges + 60 singles = 212 comparisons, 211 logical joins: the same
    // shape and size as the minified predicate that motivated #11510, and far
    // past the 48-level recursion cap.
    let module = lower_src(&range_predicate(76, 60));
    assert_eq!(
        return_type(&module, "YF6"),
        Type::Boolean,
        "every leaf is a comparison, so the whole chain is Boolean"
    );
}

#[test]
fn deep_string_or_chain_keeps_string_type() {
    let terms: Vec<String> = (0..120).map(|i| format!("\"s{i}\"")).collect();
    let src = format!("function pick(){{return {}}}\n", terms.join("||"));
    assert_eq!(return_type(&lower_src(&src), "pick"), Type::String);
}

#[test]
fn deep_chain_with_one_mismatched_leaf_stays_any() {
    // Soundness: a chain can evaluate to ANY leaf, so a single non-Boolean
    // leaf anywhere (here: last) must still widen the result to `Any`.
    let src = range_predicate(76, 60).replace("}\n", "||q}\n");
    assert_eq!(return_type(&lower_src(&src), "YF6"), Type::Any);
}

#[test]
fn grouped_logical_chain_keeps_boolean_type() {
    let groups: Vec<String> = (0..10)
        .map(|g| {
            let terms: Vec<String> = (0..10).map(|i| format!("q==={}", g * 10 + i)).collect();
            format!("({})", terms.join("||"))
        })
        .collect();
    let src = format!("function grouped(q){{return {}}}\n", groups.join("&&"));
    assert_eq!(return_type(&lower_src(&src), "grouped"), Type::Boolean);
}

fn flat_or_chain(terms: usize) -> String {
    let terms: Vec<String> = (0..terms).map(|i| format!("q==={i}")).collect();
    format!("function big(q){{return {}}}\n", terms.join("||"))
}

#[test]
fn flat_chain_just_past_node_cap_still_resolves() {
    // 300 terms = 599 nodes, over `INFER_LOGICAL_CHAIN_NODE_CAP`. The pairwise
    // fallback peels one join per recursion level and re-walks the left
    // sub-chain, which fits the budget after ~44 peels (< the 48-level cap).
    assert_eq!(
        return_type(&lower_src(&flat_or_chain(300)), "big"),
        Type::Boolean
    );
}

#[test]
fn flat_chain_far_past_node_cap_degrades_to_any() {
    // The work stays bounded (#5258): at most `INFER_TYPE_RECURSION_CAP` peels
    // of at most `INFER_LOGICAL_CHAIN_NODE_CAP` nodes each. A 480-term chain
    // needs more peels than the depth cap allows and answers the sound `Any`.
    assert_eq!(
        return_type(&lower_src(&flat_or_chain(480)), "big"),
        Type::Any
    );
}

#[test]
fn wide_shallow_tree_past_node_cap_stays_boolean() {
    // Over budget but shallow: the pairwise fallback still resolves it, so
    // the walk never types anything less precisely than the recursive rule.
    let groups: Vec<String> = (0..30)
        .map(|g| {
            let terms: Vec<String> = (0..10).map(|i| format!("q==={}", g * 10 + i)).collect();
            format!("({})", terms.join("||"))
        })
        .collect();
    let src = format!("function wide(q){{return {}}}\n", groups.join("||"));
    assert_eq!(return_type(&lower_src(&src), "wide"), Type::Boolean);
}
