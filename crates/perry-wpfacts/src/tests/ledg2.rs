//! LEDG2: the ledger's root sets. A site refused by two roots counts in
//! `mass_any` of both, in `mass_only` of neither and once in the pair
//! table; a site with one root counts in that root's `mass_only`.
use super::*;
use crate::hold_ledger::{tally, Refused};

fn site(roots: &[u32], full: &[u32], store: bool) -> Refused {
    Refused {
        roots: roots.to_vec(),
        full: full.to_vec(),
        causes: Vec::new(),
        part: 0,
        store,
        why: None,
    }
}

#[test]
fn root_sets_count_only_pairs_and_sizes() {
    let t = tally(&[
        site(&[3, 7], &[3, 7], false),
        site(&[3], &[3], true),
        // Recorded {7}, but root 3 escaped its token again: a pair.
        site(&[7], &[3, 7], true),
        site(&[1, 2, 3, 4], &[1, 2, 3, 4], false),
        site(&[], &[], false),
    ]);
    let m = |r: u32| t.mass.get(&r).copied().unwrap_or_default();
    assert_eq!((m(3).any, m(7).any), (3, 2));
    assert_eq!((m(3).only, m(3).stores_only), (1, 1));
    assert_eq!(m(7).only, 0);
    assert_eq!(t.pairs.len(), 1);
    assert_eq!(t.pairs.get(&(3, 7)), Some(&(2, 1)));
    assert_eq!(t.sizes, [1, 2, 0, 1, 0]);
    assert_eq!(t.widened, 1);
    // First arrival is unchanged: the smallest recorded root.
    assert_eq!((m(3).first[0], m(7).first[0], m(1).first[0]), (2, 1, 1));
}

const SRC: &str = "declare function a(x: any): void;\ndeclare function b(x: any): void;\nclass P { v = 1; w = 3; }\nclass Q { v = 2; }\nconst p = new P();\nconst q = new Q();\na(p);\nb(q);\nconst o = Math.random() > 0.5 ? p : q;\nconsole.log(o.v, p.w);\n";

#[test]
fn a_site_on_two_holds_is_a_pair() {
    let dir = std::env::temp_dir().join(format!("ledg2_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let out = dir.join("ledger.tsv");
    std::env::set_var("PERRY_FACTS_LEDGER", &out);
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(SRC, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    let _ = crate::compute(&[(path, &hir)], &FactsConfig::default());
    std::env::remove_var("PERRY_FACTS_LEDGER");
    let read =
        |ext: &str| std::fs::read_to_string(format!("{}{ext}", out.display())).expect("written");
    let (tsv, only, summary) = (read(""), read(".only"), read(".summary"));
    let _ = std::fs::remove_dir_all(&dir);
    eprintln!("{tsv}\n{only}\n{summary}");
    assert!(tsv.starts_with("seq\tkind\treason\twhere\tmass_first\tmass_any\tcore\taddons\trest\tstores_first\tmass_only\tstores_only\n"));
    // `o.v` is refused by both calls' holds: one pair line.
    assert!(summary.contains("LEDGER pair1 sites 1 "), "{summary}");
    // `p.w` is refused by `a`'s hold alone.
    let rows: Vec<Vec<&str>> = only
        .lines()
        .skip(1)
        .map(|l| l.split('\t').collect())
        .collect();
    assert!(
        !rows.is_empty() && rows.iter().all(|c| c[10].parse::<usize>().unwrap() > 0),
        "{only}"
    );
    assert!(summary.contains("LEDGER proven reads "), "{summary}");
}
