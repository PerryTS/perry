//! Census only: the write `settle_native_given` makes for a prototype give is attributed to the
//! operation that gave the prototype (its receiver node, or its call), and the given value is
//! kept as `proto`. A top receiver or target makes the any-key write derived, not a root seed.
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

/// The any-key "E6 prototype given" writes on the token named `holder`: (source, call, proto).
fn given_writes(pf: &ProgramFacts, holder: &str) -> Vec<(String, String, String)> {
    let lines: Vec<&String> = pf.diagnostics.iter().flat_map(|d| d.1.iter()).collect();
    let tok = if holder == "TOP" {
        "0".to_string()
    } else {
        lines
            .iter()
            .find(|l| {
                l.starts_with("{\"record\":\"token\"")
                    && field(l, "name").as_deref() == Some(holder)
            })
            .and_then(|l| field(l, "id"))
            .expect("holder token")
    };
    let opener = lines.iter().any(|l| {
        l.starts_with("{\"record\":\"birth-opener\"")
            && field(l, "holder").as_deref() == Some(&tok)
            && l.contains("E6 prototype given")
    });
    assert!(opener, "precondition: the opener `E6 prototype given`");
    lines
        .iter()
        .filter(|l| {
            l.starts_with("{\"record\":\"birth-write\"")
                && field(l, "token").as_deref() == Some(&tok)
                && field(l, "key").as_deref() == Some("null")
        })
        .map(|l| {
            (
                field(l, "source").unwrap(),
                field(l, "call").unwrap(),
                field(l, "proto").unwrap_or_else(|| "null".into()),
            )
        })
        .collect()
}

fn node_is_top(pf: &ProgramFacts, id: &str) -> bool {
    pf.diagnostics.iter().flat_map(|d| d.1.iter()).any(|l| {
        l.starts_with("{\"record\":\"node\"")
            && field(l, "id").as_deref() == Some(id)
            && field(l, "top").as_deref() == Some("true")
    })
}

const HEAD: &str = r#"
declare function external(x: any): void;
declare function unknown(): any;
const v = { [String(Math.random())]: 1 };
external(Object);
external(v);
"#;

/// F1: a store `type[key] = val` through a top receiver that holds `Object`, with an unknown key,
/// gives `val` as a prototype. The write is attributed to the (top) receiver; `proto` is `val`.
/// The one receiver joins `unknown()` (top) with an `Object` that has not escaped, so the set keeps
/// the builtin beside top. A load of `Object` alone is exactly the builtin (the global object is
/// kept): its give is C1's known receiver, and an escaped `Object` is absorbed into top.
#[test]
fn computed_store_give_names_its_top_receiver() {
    let head = HEAD.replace("external(Object);\n", "");
    let src = format!(
        "{head}function setCached(type: any, key: string, val: any) {{ type[key] = val; }}\n\
         setCached(Math.random() < 0.5 ? unknown() : Object, String(Math.random()), v);\n"
    );
    let pf = census(&src);
    let w = given_writes(&pf, "builtin:Object");
    assert!(!w.is_empty(), "no any-key write");
    for (source, call, proto) in &w {
        assert_ne!(source, proto, "the source is the given value: {w:?}");
        assert!(
            node_is_top(&pf, source),
            "source {source} is not top: {w:?}"
        );
        assert_eq!(call, &u32::MAX.to_string());
    }
    assert!(w.iter().any(|x| x.2 != "null"), "no proto field: {w:?}");
}

/// F2: `Object.setPrototypeOf(unknown(), v)` gives by call; the write names its (top) target.
#[test]
fn set_prototype_of_give_names_its_top_target() {
    let src = format!("{HEAD}Object.setPrototypeOf(unknown(), v);\n");
    let pf = census(&src);
    let w = given_writes(&pf, "TOP");
    assert!(!w.is_empty(), "no any-key write");
    for (source, _, proto) in &w {
        assert_ne!(source, proto, "{w:?}");
        assert!(
            node_is_top(&pf, source),
            "source {source} is not top: {w:?}"
        );
    }
    assert!(w.iter().any(|x| x.2 != "null"), "no proto field: {w:?}");
}

/// C1: the receiver is the program own `Object` (not top): the write keeps a non-top source.
#[test]
fn computed_store_give_on_a_known_receiver_keeps_a_root() {
    let src = "const v = { [String(Math.random())]: 1 };\n\
        function setOn(o: any, key: string, val: any) { o[key] = val; }\n\
        setOn(Object, String(Math.random()), v);\n\
        console.log(1);\n";
    let pf = census(src);
    let w = given_writes(&pf, "builtin:Object");
    assert!(!w.is_empty(), "no any-key write");
    for (source, _, _) in &w {
        assert!(!node_is_top(&pf, source), "source {source} is top: {w:?}");
    }
}

/// C2: `Object.setPrototypeOf(Object, v)` (a known target) stays a root: non-top source.
#[test]
fn set_prototype_of_a_known_target_keeps_a_root() {
    let src = "const v = { [String(Math.random())]: 1 };\nObject.setPrototypeOf(Object, v);\n\
        console.log((Object as any).foo);\n";
    let pf = census(src);
    let w = given_writes(&pf, "builtin:Object");
    assert!(!w.is_empty(), "no any-key write");
    for (source, _, _) in &w {
        assert!(!node_is_top(&pf, source), "source {source} is top: {w:?}");
    }
}
