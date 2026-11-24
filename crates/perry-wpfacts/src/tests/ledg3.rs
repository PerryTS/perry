//! LEDG3: the escape-predecessor graph. A token reached by two causes
//! through an already-escaped token is freed by neither root alone and by
//! the pair; a token only one root reaches is freed by that root.
use super::*;
use crate::hold_ledger::{v_node, v_root, v_tok, Freed, Graph, Refused};

fn site(causes: &[u64], store: bool) -> Refused {
    Refused {
        roots: vec![0],
        full: vec![0],
        causes: causes.to_vec(),
        part: 0,
        store,
        why: None,
        site: String::new(),
    }
}

#[test]
fn two_causes_on_an_escaped_token_free_only_as_a_pair() {
    // Root 0 escapes a; a's field node f sinks b; b's field g sinks s.
    // Root 1 escapes c; c's field h sinks b again (b already escaped: the
    // escape stops there, the edge is still recorded).
    let edges = [
        (v_root(0), v_tok(10)),
        (v_tok(10), v_node(100)),
        (v_node(100), v_tok(11)),
        (v_tok(11), v_node(101)),
        (v_node(101), v_tok(12)),
        (v_root(1), v_tok(13)),
        (v_tok(13), v_node(102)),
        (v_node(102), v_tok(11)),
        // Root 2 alone reaches d.
        (v_root(2), v_tok(14)),
    ];
    let g = Graph::new(edges.iter());
    let base = g.labels(&|_| false);
    let refused = [
        site(&[v_tok(12)], false),
        site(&[v_tok(11)], true),
        site(&[v_tok(14)], false),
        // A cause no root reaches: never freed.
        site(&[v_tok(99)], false),
    ];
    let f = |s: &dyn Fn(u32) -> bool| g.freed(&refused, &base, s);
    let a = f(&|r| r == 0);
    assert_eq!((a.sites, a.first), (0, 2));
    // Root 1 takes over both.
    assert_eq!(a.reroot, Some((1, 2)));
    assert_eq!(f(&|r| r == 1).sites, 0);
    assert_eq!(
        f(&|r| r <= 1),
        Freed {
            sites: 2,
            stores: 1,
            first: 2,
            reroot: None
        }
    );
    assert_eq!(f(&|r| r == 2).sites, 1);
}

const SRC: &str = "declare function a(x: any): void;\ndeclare function b(x: any): void;\nclass S { v = 1; }\nclass Q { s = new S(); }\nconst q = new Q();\na({ x: q });\nb({ y: q });\nconsole.log(q.s.v);\n";

#[test]
fn a_cascade_through_an_escaped_token_is_not_freed_by_its_first_root() {
    let dir = std::env::temp_dir().join(format!("ledg3_{}", std::process::id()));
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
    let (freed, summary) = (read(".freed"), read(".summary"));
    let _ = std::fs::remove_dir_all(&dir);
    eprintln!("{freed}\n{summary}");
    assert!(summary.contains("LEDGER graph vertices "), "{summary}");
    let rows: Vec<Vec<&str>> = freed
        .lines()
        .skip(1)
        .filter(|l| l.starts_with("root\t"))
        .map(|l| l.split('\t').collect())
        .collect();
    assert!(!rows.is_empty(), "{freed}");
    // `q.s` and `s.v` are reached by both calls: no single root frees them,
    // though `a` (the first) holds `s.v` alone in the recorded sets.
    assert!(rows.iter().all(|c| c[1] == "0"), "{freed}");
    assert!(rows.iter().any(|c| c[7] != "0"), "{freed}");
}
