//! An array's helper nodes belong to the array, not to the call: what
//! iterating Array `t` yields (`Model::arr_iterated`) and the `constructor`
//! ArraySpeciesCreate reads off `t` (`Model::species_ctor`) are pure
//! functions of `t`, so one node per token holds what a node per (call, `t`)
//! held. Fifty calls on one array make one of each.

use crate::model::{TmplKind, TokKind};

const CALLS: usize = 50;

fn source() -> String {
    // `key` is unknown: `(xs as any)[key]()` is `array_method`'s
    // computed-key method, whose array is made through `xs.constructor`.
    let mut s = String::from(
        "const xs = [{ a: 1 }, { b: 2 }];\nconst key: string = process.argv[2];\nlet n = 0;\n",
    );
    for i in 0..CALLS {
        match i % 5 {
            0 => s.push_str(&format!(
                "const v{i} = xs.values().next().value; n += v{i} ? 1 : 0;\n"
            )),
            1 => s.push_str(&format!(
                "const v{i} = (xs as any)[key](); n += v{i} ? 1 : 0;\n"
            )),
            2 => s.push_str(&format!(
                "const v{i} = (xs as any)[key](1); n += v{i} ? 1 : 0;\n"
            )),
            3 => s.push_str(&format!(
                "const v{i} = xs.entries().next().value; n += v{i} ? 1 : 0;\n"
            )),
            _ => s.push_str(&format!(
                "const v{i} = xs[Symbol.iterator]().next().value; n += v{i} ? 1 : 0;\n"
            )),
        }
    }
    s.push_str("console.log(n);\n");
    s
}

#[test]
fn one_array_used_by_fifty_calls_has_one_helper_node_of_each_kind() {
    let src = source();
    let module = perry_parser::parse_typescript(&src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let mut cfg = crate::FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    let sorted = [("t.ts", &hir)];
    let (mut g, _, _) = crate::generate(&sorted, &cfg, None, None, None, std::time::Instant::now());
    g.m.solve();
    let m = &g.m;
    let count = |l: &str| m.labels.iter().filter(|&&x| x == l).count();
    let iterated: Vec<usize> = (0..m.labels.len())
        .filter(|&n| m.labels[n] == "arr-iterated")
        .collect();
    assert_eq!(
        iterated.len(),
        1,
        "arr-iterated nodes: one per array token, not per call"
    );
    assert_eq!(count("array-species-receiver"), 1, "species receiver nodes");
    assert_eq!(
        count("array-species-constructor"),
        1,
        "species constructor nodes"
    );
    // The one node holds exactly what a call's node held: the array's
    // elements and its hole's `undefined` (both per token).
    let recv = (0..m.labels.len())
        .find(|&n| m.labels[n] == "array-species-receiver")
        .unwrap();
    let mut toks = Vec::new();
    m.nodes[recv].pts.sorted_into(&mut toks);
    assert_eq!(toks.len(), 1, "the receiver node holds xs");
    let t = toks[0];
    assert!(
        matches!(m.toks[t as usize], TokKind::Obj(tm, _) if matches!(m.tmpls[tm as usize].kind, TmplKind::Arr { .. }))
    );
    let it = iterated[0];
    let fa = g.m.field_any(t);
    let h = g.m.hole(t);
    let m = &g.m;
    let set = |n: usize| {
        let mut v = Vec::new();
        m.nodes[n].pts.sorted_into(&mut v);
        v
    };
    let mut want = set(fa as usize);
    want.extend(set(h as usize));
    want.sort_unstable();
    want.dedup();
    let got = set(it);
    assert!(
        !got.is_empty() || m.nodes[it].prim != 0,
        "what iterating xs yields is empty"
    );
    assert_eq!(got, want, "iterated = elements and hole");
    assert_eq!(
        m.nodes[it].prim,
        m.nodes[fa as usize].prim | m.nodes[h as usize].prim
    );
}
