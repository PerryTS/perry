//! WHYHOLD1: `PERRY_FACTS_WHY=1` records each value's first arrival; the
//! first realm hold walks the callee value back to its seed (the record
//! API `provenance::take_chain`, not stderr).
use super::*;

const SRC: &str = "declare function sink(x: any): void;\nfunction make() {\n  return sink;\n}\nconst f = make();\nf(1);\n";

#[test]
fn chain_from_hold_reaches_the_seed() {
    std::env::set_var("PERRY_FACTS_WHY", "1");
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(SRC, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    let _ = crate::provenance::take_chain();
    let _ = crate::compute(&[(path, &hir)], &FactsConfig::default());
    std::env::remove_var("PERRY_FACTS_WHY");
    let c = crate::provenance::take_chain();
    for h in &c {
        eprintln!("{h}");
    }
    // The held call `f(1)`: its callee value is ⊤ at the callee node, which
    // came from the call result of `make()`, from `make`'s return, from the
    // read of the undeclared-body `sink` (⊤, the program's TOPVAL seed).
    let v: Vec<(&str, &str, &str)> = c.iter().map(|h| (h.label.as_str(), h.value.as_str(), h.end)).collect();
    assert_eq!(c.len(), 4, "hops: {v:?}");
    assert!(c.iter().all(|h| h.value == "t0:Top" && h.solve == c[0].solve), "{v:?}");
    assert_eq!(c[0].label, "local");
    assert_eq!((c[1].label.as_str(), c[1].site.as_str()), ("callret", "c0/s0"), "{v:?}");
    assert!(c[2].label == "ret" && c[2].func.ends_with(":make"), "{v:?}");
    assert!(c[2].via.starts_with("copy<-n") && c[2].via.contains("edge@gen/walk.rs"), "{:?}", c[2]);
    assert_eq!((c[3].label.as_str(), c[3].end), ("TOPVAL", "seed"), "{v:?}");
    assert!(c[..3].iter().all(|h| h.end.is_empty()), "{v:?}");
    for w in c.windows(2) {
        assert!(w[0].via.contains(&format!("<-n{}", w[1].node)), "hop source is the next node: {:?}", w);
        assert!(w[0].work >= w[1].work, "{:?}", w);
    }
}
