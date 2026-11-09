//! Census only: a spread of the top value is propagation of the top its source value holds, so the
//! `spread of an unmodeled object` injection is attributed to the source value node, not to the
//! target node whose rule runs. An opaque source that is not top (a built-in function) stays an origin.
use super::*;

fn census(src: &str) -> ProgramFacts {
    let cfg = FactsConfig {
        diagnostics: true,
        birth_census: true,
        ..Default::default()
    };
    facts_with(src, cfg)
}

fn field(l: &str, k: &str) -> Option<String> {
    let s = l.find(&format!("\"{k}\":"))? + k.len() + 3;
    Some(
        l[s..]
            .split([',', '}'])
            .next()?
            .trim_matches('"')
            .to_string(),
    )
}

fn lines(pf: &ProgramFacts) -> Vec<&String> {
    pf.diagnostics.iter().flat_map(|d| d.1.iter()).collect()
}

fn node_is_top(pf: &ProgramFacts, id: &str) -> bool {
    lines(pf).iter().any(|l| {
        l.starts_with("{\"record\":\"node\"")
            && field(l, "id").as_deref() == Some(id)
            && field(l, "top").as_deref() == Some("true")
    })
}

/// The sources of the `spread of an unmodeled object` injections.
fn spread_sources(pf: &ProgramFacts) -> Vec<String> {
    let v: Vec<String> = lines(pf)
        .iter()
        .filter(|l| {
            l.starts_with("{\"record\":\"birth-injection\"")
                && l.contains("\"why\":\"spread of an unmodeled object\"")
        })
        .map(|l| field(l, "source").unwrap())
        .collect();
    assert!(!v.is_empty(), "precondition: a spread injection");
    v
}

const K: &str = "class K { m(): number { return 1; } }\ndeclare function external(x: any): void;\n";

/// F: JSON data given to unknown code, then `Object.assign(ud, def.extras)` (the three.js shape).
/// The injection names `def.extras` (top), not `ud` (never top).
#[test]
fn spread_of_escaped_data_names_its_source_value() {
    let src = format!(
        "{K}const def: any = JSON.parse(String(Math.random()));\nexternal(def);\n\
         const ud: any = {{ k: new K() }};\nObject.assign(ud, def.extras);\nconsole.log(ud.k.m());\n"
    );
    let pf = census(&src);
    for s in spread_sources(&pf) {
        assert!(node_is_top(&pf, &s), "source {s} is not top");
    }
}

/// C (named control): a spread of a parameter of a function given to unknown code. The injection
/// is re-attributed to the parameter value (top), not removed: `ud.k.m()` stays unproven.
#[test]
fn spread_of_a_given_parameter_stays_unproven() {
    let src = format!(
        "{K}function f(p: any) {{ const ud: any = {{ k: new K() }}; Object.assign(ud, p); return ud.k.m(); }}\nexternal(f);\n"
    );
    let pf = census(&src);
    for s in spread_sources(&pf) {
        assert!(node_is_top(&pf, &s), "source {s} is not top");
    }
    let m: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.is_call && s.key == "m")
        .collect();
    assert_eq!(m.len(), 1, "{}", pf.dump_text());
    assert!(matches!(m[0].verdict, Verdict::No(_)), "{:?}", m[0].verdict);
}

/// Control: an opaque source that is not top (a built-in function) stays an origin: its injection
/// keeps the target node as its cause, which is not top.
#[test]
fn spread_of_an_opaque_builtin_stays_an_origin() {
    let src = format!("{K}const a: any = {{ k: new K(), ...Math.max }};\nconsole.log(a.k.m());\n");
    let pf = census(&src);
    for s in spread_sources(&pf) {
        assert!(!node_is_top(&pf, &s), "source {s} is top");
    }
}
