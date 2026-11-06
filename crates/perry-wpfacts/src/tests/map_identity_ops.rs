//! Native Map identity operations keep ordinary method lookup and operand evaluation.
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
fn native_map_identity_does_not_run_receiver_members() {
    let mut failures = Vec::new();
    for operation in ["map.has(0)", "map.delete(0)", "map.clear()"] {
        let src = format!(
            r#"
declare function external(x: unknown): void;
class Payload {{ n = 1; }}
function run() {{
    const payload = new Payload();
    const map: Map<unknown, unknown> = new Map();
    map.saved = payload;
    map.unrelated = () => external(payload);
    {operation};
    console.log(payload.n); // PAYLOAD_N
}}
run();
"#
        );
        if let Err(error) = payload_site(&src, true) {
            failures.push(format!("{operation}: {}", error.lines().next().unwrap()));
        }
    }
    assert!(
        failures.is_empty(),
        "unproven native Map forms: {failures:?}"
    );
}

#[test]
fn native_map_keys_are_not_coerced_or_called() {
    let mut failures = Vec::new();
    for operation in ["map.has(key)", "map.delete(key)"] {
        let src = format!(
            r#"
declare function external(x: unknown): void;
class Payload {{ n = 1; }}
function run() {{
    const payload = new Payload();
    const key = {{ toString() {{ external(payload); return "x"; }}, unrelated() {{ external(payload); }} }};
    const map: Map<unknown, unknown> = new Map();
    {operation};
    console.log(payload.n); // PAYLOAD_N
}}
run();
"#
        );
        if let Err(error) = payload_site(&src, true) {
            failures.push(format!("{operation}: {}", error.lines().next().unwrap()));
        }
    }
    assert!(failures.is_empty(), "unproven identity keys: {failures:?}");
}

#[test]
fn native_map_function_key_does_not_publish_its_saved_value() {
    payload_site(
        r#"
class Payload { n = 1; }
function run() {
    const payload = new Payload();
    const key = Array.isArray;
    key.saved = payload;
    const map: Map<unknown, unknown> = new Map();
    map.has(key);
    console.log(payload.n); // PAYLOAD_N
}
run();
"#,
        true,
    )
    .unwrap();
}

#[test]
fn native_map_keeps_callback_and_key_expression_effects() {
    for operation in [
        "map.forEach(value => external(value))",
        "map.has(external(payload))",
    ] {
        let src = format!(
            r#"
declare function external(x: unknown): void;
class Payload {{ n = 1; }}
function run() {{
    const payload = new Payload();
    const map: Map<unknown, unknown> = new Map();
    map.set("x", payload);
    {operation};
    console.log(payload.n); // PAYLOAD_N
}}
run();
"#
        );
        payload_site(&src, false).unwrap();
    }
}

#[test]
fn native_map_keeps_own_method_and_getter_effects() {
    for operation in [
        "map.has = () => { external(payload); return false; }; map.has(0)",
        "map.delete = () => { external(payload); return false; }; map.delete(0)",
        "Object.defineProperty(map, 'has', { get() { external(payload); return () => false; } }); map.has(0)",
    ] {
        let src = format!(r#"
declare function external(x: unknown): void;
class Payload {{ n = 1; }}
function run() {{
    const payload = new Payload();
    const map: Map<unknown, unknown> = new Map();
    {operation};
    console.log(payload.n); // PAYLOAD_N
}}
run();
"#);
        payload_site(&src, false).unwrap();
    }
}

#[test]
fn native_map_unknown_receiver_is_not_decided_by_its_annotation() {
    // `unknownMap` is an unseen module's global.
    payload_site(
        &unseen(r#"
declare const unknownMap: unknown;
class Payload { n = 1; }
function run() {
    const payload = new Payload();
    const map: Map<unknown, unknown> = unknownMap;
    map.has(payload);
    console.log(payload.n); // PAYLOAD_N
}
run();
"#),
        false,
    )
    .unwrap();
}

#[test]
fn native_map_override_result_is_not_forced_to_boolean() {
    payload_site(
        r#"
declare function external(x: unknown): void;
class Payload { n = 1; }
function run() {
    const payload = new Payload();
    const map: Map<unknown, unknown> = new Map();
    map.has = () => payload;
    const value = map.has(0);
    external(value);
    console.log(payload.n); // PAYLOAD_N
}
run();
"#,
        false,
    )
    .unwrap();
}
