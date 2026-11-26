//! The seed census (`PERRY_FACTS_STOP_ON_TOP_LIST`, `topstop::Census`): a
//! diagnostic for ordering work. No seed stops the solve; each seed key is
//! listed once, every first ⊤ arrival is booked to the seed its ⊤ came from,
//! and a seed whose rule input already held ⊤ is marked downstream.
use super::*;

fn census(src: &str) -> String {
    let ast = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&ast, "t", "t.ts").expect("lowers");
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    crate::topstop::census_capture_on(40, 0);
    let _ = compute(&[("t.ts".to_string(), &hir)], &cfg);
    let mut v = crate::topstop::take_captured();
    assert_eq!(v.len(), 1, "one census block: {v:#?}");
    v.swap_remove(0)
}

/// Two independent seeds (the own values of a value no model describes, the
/// result of `new Function(source)`) and one consequence: the call of that
/// unknown function may throw anything, a seed whose rule input (the callee)
/// already holds the second seed's ⊤.
const TWO_ROOTS_ONE_DOWNSTREAM: &str = "const g: any = globalThis;\n\
const a: any = g[String(Date.now())];\n\
const d = Object.getOwnPropertyDescriptor(a, \"k\");\n\
const b: any = new Function(process.argv[2]);\n\
const r = b();\n\
console.log(d, r);\n";

#[test]
fn census_lists_two_roots_and_one_downstream_seed() {
    let b = census(TWO_ROOTS_ONE_DOWNSTREAM);
    let seeds: Vec<&str> = b
        .lines()
        .filter(|l| l.starts_with("CENSUS-SEED "))
        .collect();
    assert_eq!(seeds.len(), 3, "{b}");
    let of = |what: &str| {
        seeds
            .iter()
            .find(|l| l.contains(what))
            .unwrap_or_else(|| panic!("no seed {what}:\n{b}"))
            .split_whitespace()
            .nth(1)
            .unwrap()
            .to_string()
    };
    let (desc, func, throw) = (
        of("own values of an unmodeled object"),
        of("new of a function constructor"),
        of("a call of an unknown function value may throw anything"),
    );
    let line = |id: &str| {
        seeds
            .iter()
            .find(|l| l.split_whitespace().nth(1) == Some(id))
            .unwrap()
            .to_string()
    };
    assert!(line(&desc).contains(" indep "), "{b}");
    assert!(line(&func).contains(" indep "), "{b}");
    assert!(line(&throw).contains(&format!(" down={func} ")), "{b}");
    // Reach: every seed holds nodes; the root's total covers its consequence.
    let reach = |id: &str, f: &str| -> u64 {
        let l = b
            .lines()
            .find(|l| l.starts_with(&format!("CENSUS-REACH {id} ")))
            .unwrap_or_else(|| panic!("no reach {id}:\n{b}"));
        l.split_whitespace()
            .find_map(|w| w.strip_prefix(&format!("{f}=")))
            .unwrap()
            .parse()
            .unwrap()
    };
    for id in [&desc, &func, &throw] {
        assert!(reach(id, "nodes") > 0, "{b}");
    }
    assert_eq!(
        reach(&func, "total"),
        reach(&func, "nodes") + reach(&throw, "total"),
        "{b}"
    );
    assert_eq!(reach(&desc, "total"), reach(&desc, "nodes"), "{b}");
    assert!(b.contains("CENSUS-END why=solve end "), "{b}");
}
