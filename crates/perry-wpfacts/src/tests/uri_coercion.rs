//! URI operands use ToString hooks; unrelated members never run.
use super::*;

fn check_site(src: &str, marker: &str, key: &str, call: bool, proven: bool) -> Result<(), String> {
    let at = src.find(marker).expect("named marker");
    let start = src[..at].rfind('\n').map_or(0, |n| n + 1) as u32;
    let pf = facts(src);
    let sites: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| {
            s.is_call == call && s.key == key && (start..at as u32).contains(&s.byte_offset)
        })
        .collect();
    if sites.is_empty() {
        return Err(format!("missing {marker}/{key}\n{}", pf.dump_text()));
    }
    for s in sites {
        if !matches!(s.verdict, Verdict::No(_)) != proven {
            return Err(format!(
                "{marker}/{key}: {:?}\n{}",
                s.verdict,
                pf.dump_text()
            ));
        }
    }
    Ok(())
}

fn site(src: &str, marker: &str, key: &str, call: bool, proven: bool) {
    if let Err(error) = check_site(src, marker, key, call, proven) {
        panic!("{error}");
    }
}

#[test]
fn uri_tsc_original_for_each() {
    site(
        r#"
class Value { n = 1; }
const links = { table: new Map() };
links.table.set("x", new Value());
const f = Array.isArray;
f.saved = links;
encodeURI(f);
const resolutionKind = "table";
const original = links["table"];
original.forEach(v => console.log(v.n)); // ORIGINAL
"#,
        "// ORIGINAL",
        "forEach",
        true,
        true,
    );
}

#[test]
fn uri_saved_payload_n() {
    site(
        r#"
class Payload { n = 1; }
const payload = new Payload();
const f = Array.isArray;
f.saved = payload;
encodeURI(f);
console.log(payload.n); // SAVED
"#,
        "// SAVED",
        "n",
        false,
        true,
    );
}

#[test]
fn uri_unrelated_method_payload_n() {
    site(
        r#"
declare function external(x: any): void;
class Payload { n = 1; }
const payload = new Payload();
const arg = { toString() { return "x"; }, unrelated() { external(payload); } };
encodeURI(arg);
console.log(payload.n); // UNRELATED
"#,
        "// UNRELATED",
        "n",
        false,
        true,
    );
}

#[test]
fn uri_four_coercion_forms_on_each_operation() {
    let forms = [
        r#"toString() { return "x"; }, unrelated() { external(payload); }"#,
        r#"toString: undefined, valueOf() { return "x"; }, unrelated() { external(payload); }"#,
        r#"[Symbol.toPrimitive](hint: any) { return "x"; }, unrelated() { external(payload); }"#,
        r#"toString() { return "x"; }, get unrelated() { return 0; }, other() { external(payload); }"#,
    ];
    let mut failures = Vec::new();
    for op in [
        "encodeURI",
        "decodeURI",
        "encodeURIComponent",
        "decodeURIComponent",
    ] {
        for (i, form) in forms.iter().enumerate() {
            let src = format!(
                r#"
declare function external(x: any): void;
class Payload {{ n = 1; }}
const payload = new Payload();
const arg = {{ {form} }};
{op}(arg);
console.log(payload.n); // MATRIX-{op}-{i}
"#
            );
            if let Err(error) = check_site(&src, &format!("// MATRIX-{op}-{i}"), "n", false, true) {
                failures.push(format!("{op}-{i}: {}", error.lines().next().unwrap()));
            }
        }
    }
    assert!(failures.is_empty(), "unproven forms: {failures:?}");
}

#[test]
fn unknown_receiver_changes_n() {
    site(
        r#"
declare function input(): any;
const o = { n: 1, method: input() };
o.method();
console.log(o.n); // UNKNOWN-RECEIVER
"#,
        "// UNKNOWN-RECEIVER",
        "n",
        false,
        false,
    );
}

#[test]
fn uri_to_string_leaks_n() {
    for op in [
        "encodeURI",
        "decodeURI",
        "encodeURIComponent",
        "decodeURIComponent",
    ] {
        let src = format!(
            r#"
declare function external(x: any): void;
const o = {{ n: 1, toString() {{ external(o); return "x"; }} }};
{op}(o);
console.log(o.n); // TO-STRING
"#
        );
        site(&src, "// TO-STRING", "n", false, false);
    }
}

#[test]
fn uri_value_of_leaks_n() {
    for op in [
        "encodeURI",
        "decodeURI",
        "encodeURIComponent",
        "decodeURIComponent",
    ] {
        let src = format!(
            r#"
declare function external(x: any): void;
const o = {{ n: 1, toString: undefined, valueOf() {{ external(o); return "x"; }} }};
{op}(o);
console.log(o.n); // VALUE-OF
"#
        );
        site(&src, "// VALUE-OF", "n", false, false);
    }
}

#[test]
fn uri_unknown_to_string_stays_unproven() {
    site(
        r#"
declare function input(): any;
const o = { n: 1, toString: input() };
encodeURI(o);
console.log(o.n); // UNKNOWN-TO-STRING
"#,
        "// UNKNOWN-TO-STRING",
        "n",
        false,
        false,
    );
}
