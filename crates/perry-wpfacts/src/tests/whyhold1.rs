//! WHYHOLD1: `PERRY_FACTS_WHY=1` records each value's first arrival; the
//! first realm hold walks the callee value back to its seed (the record
//! API `provenance::take_chain`, not stderr).
use super::*;

const SRC: &str = "declare function sink(x: any): void;\nfunction make() {\n  return sink;\n}\nconst f = make();\nf(1);\n";

/// K8 (owner 2026-11-23, f0d8455cfb): a call of an unknown value holds the
/// realm only where code beside the program may name it (`realm_nameable`),
/// so this control runs beside an unseen module (`import "unseen-host"`).
#[test]
fn chain_from_hold_reaches_the_seed() {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(&unseen(SRC), &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    let _ = crate::provenance::take_chain();
    crate::provenance::with_env(&[("PERRY_FACTS_WHY", "1")], || {
        crate::compute(&[(path, &hir)], &FactsConfig::default())
    });
    let c = crate::provenance::take_chain();
    for h in &c {
        eprintln!("{h}");
    }
    // The held call `f(1)`: its callee value is ⊤ at the callee node, which
    // came from the call result of `make()`, from `make`'s return, from the
    // read of the undeclared-body `sink` (⊤, the program's TOPVAL seed).
    let v: Vec<(&str, &str, &str)> = c
        .iter()
        .map(|h| (h.label.as_str(), h.value.as_str(), h.end))
        .collect();
    assert_eq!(c.len(), 4, "hops: {v:?}");
    assert!(
        c.iter()
            .all(|h| h.value == "t0:Top" && h.solve == c[0].solve),
        "{v:?}"
    );
    assert_eq!(c[0].label, "local");
    assert_eq!(
        (c[1].label.as_str(), c[1].site.as_str()),
        ("callret", "c0/s0"),
        "{v:?}"
    );
    assert!(c[2].label == "ret" && c[2].func.ends_with(":make"), "{v:?}");
    assert!(
        c[2].via.starts_with("copy<-n") && c[2].via.contains("edge@gen/walk.rs"),
        "{:?}",
        c[2]
    );
    assert_eq!((c[3].label.as_str(), c[3].end), ("TOPVAL", "seed"), "{v:?}");
    assert!(c[..3].iter().all(|h| h.end.is_empty()), "{v:?}");
    for w in c.windows(2) {
        assert!(
            w[0].via.contains(&format!("<-n{}", w[1].node)),
            "hop source is the next node: {:?}",
            w
        );
        assert!(w[0].work >= w[1].work, "{:?}", w);
    }
}

fn chain_with(src: &str, env: &[(&str, &str)]) -> Vec<crate::provenance::WhyHop> {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    let _ = crate::provenance::take_chain();
    crate::provenance::with_env(env, || {
        crate::compute(&[(path, &hir)], &FactsConfig::default())
    });
    let c = crate::provenance::take_chain();
    for h in &c {
        eprintln!("{h}");
    }
    c
}

/// WHYHOLD2: `PERRY_FACTS_STOP=wide` with a name selector stops at the
/// first ⊤ arrival at the named parameter (unit tests read the record; no
/// exit), and the chain starts there.
#[test]
fn stop_wide_by_parameter_name() {
    const W: &str = "declare function sink(x: any): any;\nfunction take(rootUuid: any) {\n  return rootUuid;\n}\nconst v = sink(1);\ntake(v);\n";
    let c = chain_with(
        W,
        &[
            ("PERRY_FACTS_STOP", "wide"),
            ("PERRY_FACTS_WHY_AT", "take#rootUuid"),
        ],
    );
    let v: Vec<(&str, &str, &str)> = c
        .iter()
        .map(|h| (h.label.as_str(), h.value.as_str(), h.end))
        .collect();
    assert!(!c.is_empty(), "no wide stop");
    // The second solve stops at a summary instance's copy of the parameter
    // (selected through its original: `by=instance`).
    assert!(
        (c[0].func.ends_with(":take") || c[0].label == "inst") && c[0].value == "t0:Top",
        "{v:?}"
    );
    assert_eq!(c.last().map(|h| h.end), Some("seed"), "{v:?}");
}

/// WHYHOLD2: a load's hop names the receiver token that the load read the
/// value from (recorded at delivery, not the source's latest arrival).
/// K8 (owner 2026-11-23, f0d8455cfb): a call of an unknown value holds the
/// realm only where code beside the program may name it (`realm_nameable`),
/// so this control runs beside an unseen module (`import "unseen-host"`).
#[test]
fn load_hop_records_the_receiver() {
    const L: &str = "import \"unseen-host\";\ndeclare function sink(x: any): void;\nfunction rd(o: any) {\n  return o.f;\n}\nconst g = rd({ f: sink });\ng(1);\n";
    let c = chain_with(L, &[("PERRY_FACTS_WHY", "1")]);
    let v: Vec<(&str, &str)> = c
        .iter()
        .map(|h| (h.label.as_str(), h.via.as_str()))
        .collect();
    let h = c
        .iter()
        .find(|h| h.via.contains("|recv=t"))
        .unwrap_or_else(|| panic!("no recorded receiver: {v:?}"));
    // The receiver named is the object literal's token.
    let t: u32 = h
        .via
        .split("|recv=t")
        .nth(1)
        .and_then(|x| x.split('|').next())
        .and_then(|x| x.parse().ok())
        .expect("tok");
    assert!(
        c.iter().all(|x| x.value != "t0:Top" || x.end != "norec"),
        "{v:?}"
    );
    assert!(
        h.via.contains("load"),
        "the edge was made by a load: {:?} (tok {t})",
        h
    );
}
