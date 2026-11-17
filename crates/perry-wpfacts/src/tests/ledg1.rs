//! LEDG1: the blocker ledger (`PERRY_FACTS_LEDGER`) lists every hold, not
//! only the first: two unknown callees, each handed its own object, give
//! two roots, and each refused read of a handed object counts for its own.
use super::*;

const SRC: &str = "declare function a(x: any): void;\ndeclare function b(x: any): void;\nclass P { v = 1; }\nclass Q { w = 2; }\nconst p = new P();\nconst q = new Q();\na(p);\nb(q);\nconsole.log(p.v, q.w);\n";

#[test]
fn two_independent_holds_both_appear() {
    let dir = std::env::temp_dir().join(format!("ledg1_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let out = dir.join("ledger.tsv");
    std::env::set_var("PERRY_FACTS_LEDGER", &out);
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(SRC, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    let _ = crate::compute(&[(path, &hir)], &FactsConfig::default());
    std::env::remove_var("PERRY_FACTS_LEDGER");
    let tsv = std::fs::read_to_string(&out).expect("ledger written");
    let _ = std::fs::remove_dir_all(&dir);
    eprintln!("{tsv}");
    // seq kind reason where mass_first mass_any core addons rest stores_first
    let rows: Vec<Vec<&str>> = tsv
        .lines()
        .skip(1)
        .map(|l| l.split('\t').collect::<Vec<_>>())
        .filter(|c| c[0] != "-" && c[4].parse::<usize>().unwrap_or(0) > 0)
        .collect();
    assert!(rows.len() >= 2, "two holds with mass: {tsv}");
    assert_ne!(rows[0][0], rows[1][0], "distinct roots: {tsv}");
    // Each handed object is sunk at its own call: a root per call.
    assert!(rows.iter().all(|c| c[1] == "sink"), "{tsv}");
}
