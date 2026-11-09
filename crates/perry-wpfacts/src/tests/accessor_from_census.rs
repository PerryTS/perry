//! Census only: an accessor installed with the top function value is propagation of the top that
//! value holds, so the `accessor of unknown function value` injection is attributed to the node that
//! held the function value (top), not to the holder whose rule runs.
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

/// The sources of the `accessor of unknown function value` injections.
fn accessor_sources(pf: &ProgramFacts) -> Vec<String> {
    let v: Vec<String> = lines(pf)
        .iter()
        .filter(|l| {
            l.starts_with("{\"record\":\"birth-injection\"")
                && l.contains("\"why\":\"accessor of unknown function value\"")
        })
        .map(|l| field(l, "source").unwrap())
        .collect();
    assert!(!v.is_empty(), "precondition: an accessor injection");
    v
}

/// An accessor installed with a top function value: the injection names the node that held it.
#[test]
fn accessor_of_unknown_function_names_its_source_value() {
    let src = "declare function external(): any;\nconst f: any = external();\n\
               const o: any = {};\no.__defineGetter__(\"x\", f);\nconsole.log(o.x);\n";
    let pf = census(src);
    for s in accessor_sources(&pf) {
        assert!(node_is_top(&pf, &s), "source {s} is not top");
    }
}
