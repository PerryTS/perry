//! Computed own callees use the same key-value range as computed reads.
use super::*;

fn payload_site(src: &str, expected: bool) -> Result<(), String> {
    let at = src.find("// PAYLOAD_N").expect("named site");
    let line = src[..at].rfind('\n').map_or(0, |p| p + 1) as u32;
    let pf = facts(src);
    let sites: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| !s.is_call && s.key == "n" && (line..at as u32).contains(&s.byte_offset))
        .collect();
    if sites.is_empty() {
        return Err(format!("missing PAYLOAD_N\n{}", pf.dump_text()));
    }
    if sites
        .iter()
        .any(|s| !matches!(s.verdict, Verdict::No(_)) != expected)
    {
        return Err(format!(
            "PAYLOAD_N expected proven={expected}: {:?}\n{}",
            sites.iter().map(|s| &s.verdict).collect::<Vec<_>>(),
            pf.dump_text()
        ));
    }
    Ok(())
}

#[test]
fn indexed_callees_numeric_own_range_ignores_unrelated_named_functions() {
    let mut failures = Vec::new();
    for (receiver, key) in [
        ("[() => 0]", "0"),
        ("{ 0: () => 0 }", "0"),
        ("[() => 0]", "0n"),
        ("{ NaN: () => 0 }", "0 / 0"),
        ("{ Infinity: () => 0 }", "1 / 0"),
    ] {
        let src = format!(
            r#"
declare function external(x: unknown): void;
class Payload {{ n = 1; }}
const payload = new Payload();
const receiver = {receiver};
receiver.unrelated = () => external(payload);
let key = {key};
receiver[key]();
console.log(payload.n); // PAYLOAD_N
"#
        );
        if let Err(error) = payload_site(&src, true) {
            failures.push(format!(
                "{receiver}[{key}]: {}",
                error.lines().next().unwrap()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "unproven numeric own calls: {failures:?}"
    );
}

#[test]
fn indexed_callees_primitive_literal_keys_do_not_select_other_names() {
    let mut failures = Vec::new();
    for key in ["true", "false", "null", "undefined"] {
        let src = format!(
            r#"
declare function external(x: unknown): void;
class Payload {{ n = 1; }}
const payload = new Payload();
const receiver = {{ true: () => 0, false: () => 0, null: () => 0, undefined: () => 0 }};
receiver.unrelated = () => external(payload);
let key = {key};
receiver[key]();
console.log(payload.n); // PAYLOAD_N
"#
        );
        if let Err(error) = payload_site(&src, true) {
            failures.push(format!("{key}: {}", error.lines().next().unwrap()));
        }
    }
    assert!(
        failures.is_empty(),
        "unproven literal-key calls: {failures:?}"
    );
}

#[test]
fn indexed_callees_unknown_string_key_keeps_named_calls() {
    payload_site(
        r#"
declare function external(x: unknown): void;
class Payload { n = 1; }
const payload = new Payload();
const receiver = [() => 0];
receiver.unrelated = () => external(payload);
const key = process.argv[2];
receiver[key]();
console.log(payload.n); // PAYLOAD_N
"#,
        false,
    )
    .unwrap();
}

#[test]
fn indexed_callees_unknown_index_keeps_all_possible_calls() {
    payload_site(
        r#"
declare function external(x: unknown): void;
declare function unknownIndex(): unknown;
class Payload { n = 1; }
const payload = new Payload();
const receiver = [() => 0];
receiver.unrelated = () => external(payload);
receiver[unknownIndex()]();
console.log(payload.n); // PAYLOAD_N
"#,
        false,
    )
    .unwrap();
}

#[test]
fn indexed_callees_later_string_values_keep_named_calls() {
    payload_site(
        r#"import "unseen-host";

declare function external(x: unknown): void;
declare const choose: boolean;
class Payload { n = 1; }
const payload = new Payload();
const receiver = [() => 0];
receiver.unrelated = () => external(payload);
let key: unknown = 0;
if (choose) key = "unrelated";
receiver[key]();
console.log(payload.n); // PAYLOAD_N
"#,
        false,
    )
    .unwrap();
}

#[test]
fn indexed_callees_numeric_callback_still_runs() {
    payload_site(
        r#"
declare function external(x: unknown): void;
class Payload { n = 1; }
const payload = new Payload();
const receiver = [() => external(payload)];
let key = 0;
receiver[key]();
console.log(payload.n); // PAYLOAD_N
"#,
        false,
    )
    .unwrap();
}

#[test]
fn indexed_callees_numeric_method_keeps_its_receiver() {
    payload_site(
        r#"
declare function external(x: unknown): void;
class Payload { n = 1; }
const payload = new Payload();
const receiver = [function() { external(this.saved); }];
receiver.saved = payload;
let key = 0;
receiver[key]();
console.log(payload.n); // PAYLOAD_N
"#,
        false,
    )
    .unwrap();
}

#[test]
fn indexed_callees_inherited_numeric_accessor_still_runs() {
    payload_site(
        r#"
declare function external(x: unknown): void;
class Payload { n = 1; }
const payload = new Payload();
const prototype = { get 0() { external(payload); return () => 0; } };
const receiver = Object.create(prototype);
let key = 0;
receiver[key]();
console.log(payload.n); // PAYLOAD_N
"#,
        false,
    )
    .unwrap();
}
