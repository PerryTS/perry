//! Annex B lookup reads accessor function values; it does not invoke properties.
use super::*;

fn lookup_site(src: &str, proven: bool) -> Result<(), String> {
    let at = src.find("// LOOKUP_N").expect("named site");
    let line = src[..at].rfind('\n').map_or(0, |p| p + 1) as u32;
    let pf = facts(src);
    let sites: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| !s.is_call && s.key == "n" && (line..at as u32).contains(&s.byte_offset))
        .collect();
    if sites.is_empty() {
        return Err(format!("missing LOOKUP_N\n{}", pf.dump_text()));
    }
    if sites
        .iter()
        .any(|s| !matches!(s.verdict, Verdict::No(_)) != proven)
    {
        return Err(format!(
            "LOOKUP_N expected proven={proven}: {:?}\n{}",
            sites.iter().map(|s| &s.verdict).collect::<Vec<_>>(),
            pf.dump_text()
        ));
    }
    Ok(())
}

#[test]
fn accessor_lookup_unused_query_does_not_run_unrelated_methods() {
    let mut failures = Vec::new();
    for receiver in ["{}", "[]", "function target() {}", "/x/.exec"] {
        for method in ["__lookupGetter__", "__lookupSetter__"] {
            let src = format!(
                r#"
declare function external(x: unknown): void;
class Payload {{ n = 1; }}
const payload = new Payload();
const receiver = {receiver};
receiver.extra = () => external(payload);
receiver.{method}("missing");
console.log(payload.n); // LOOKUP_N
"#
            );
            if let Err(e) = lookup_site(&src, true) {
                failures.push(format!(
                    "{receiver}.{method}: {}",
                    e.lines().next().unwrap()
                ));
            }
        }
    }
    assert!(failures.is_empty(), "unused lookups: {failures:?}");
}

#[test]
fn accessor_lookup_returns_getter_functions_from_every_install_form() {
    let mut failures = Vec::new();
    for setup in [
        "const receiver = { get x() { return payload; } };",
        "class Holder { get x() { return payload; } } const receiver = new Holder();",
        "class Base { get x() { return payload; } } class Holder extends Base {} const receiver = new Holder();",
        "const receiver = {}; Object.defineProperty(receiver, 'x', { get: () => payload });",
        "const receiver = {}; receiver.__defineGetter__('x', () => payload);",
    ] {
        let src = format!(r#"
class Payload {{ n = 1; }}
const payload = new Payload();
{setup}
const getter = receiver.__lookupGetter__("x");
const value = getter.call(receiver);
console.log(value.n); // LOOKUP_N
"#);
        if let Err(e) = lookup_site(&src, true) { failures.push(format!("{setup}: {}",e.lines().next().unwrap())); }
    }
    assert!(failures.is_empty(), "getter values: {failures:?}");
}

#[test]
fn accessor_lookup_late_install_reaches_an_existing_query() {
    lookup_site(
        r#"
class Payload { n = 1; }
const payload = new Payload();
const receiver = {};
function query() { return receiver.__lookupGetter__("x"); }
query();
receiver.__defineGetter__("x", () => payload);
const getter = query();
const value = getter.call(receiver);
console.log(value.n); // LOOKUP_N
"#,
        true,
    )
    .unwrap();
}

#[test]
fn accessor_lookup_data_and_opposite_accessor_shadow_the_inherited_getter() {
    let mut failures = Vec::new();
    for member in ["x = 0;", "set x(value) {}"] {
        let src = format!(
            r#"
declare function external(x: unknown): void;
class Payload {{ n = 1; }}
const payload = new Payload();
class Base {{ get x() {{ external(this.payload); return 0; }} }}
class Holder extends Base {{ {member} }}
const receiver = new Holder();
receiver.payload = payload;
receiver.extra = () => external(payload);
const getter = receiver.__lookupGetter__("x");
if (getter) getter.call(receiver);
console.log(payload.n); // LOOKUP_N
"#
        );
        if let Err(e) = lookup_site(&src, true) {
            failures.push(format!("{member}: {}", e.lines().next().unwrap()));
        }
    }
    assert!(failures.is_empty(), "descriptor shadows: {failures:?}");
}

#[test]
fn accessor_lookup_real_returned_getter_calls_remain_unproven() {
    lookup_site(
        r#"
declare function external(x: unknown): void;
class Payload { n = 1; }
const payload = new Payload();
const receiver = { payload, get x() { external(this.payload); return 0; } };
const getter = receiver.__lookupGetter__("x");
getter.call(receiver);
console.log(payload.n); // LOOKUP_N
"#,
        false,
    )
    .unwrap();
}

#[test]
fn accessor_lookup_real_returned_setter_calls_remain_unproven() {
    let mut failures = Vec::new();
    for setup in [
        "const receiver = { set x(value) { external(value); } };",
        "class Holder { set x(value) { external(value); } } const receiver = new Holder();",
        "const receiver = {}; Object.defineProperty(receiver, 'x', { set: (value) => external(value) });",
        "const receiver = {}; receiver.__defineSetter__('x', (value) => external(value));",
    ] {
        let src = format!(r#"
declare function external(x: unknown): void;
class Payload {{ n = 1; }}
const payload = new Payload();
{setup}
const setter = receiver.__lookupSetter__("x");
setter.call(receiver, payload);
console.log(payload.n); // LOOKUP_N
"#);
        if let Err(e) = lookup_site(&src, false) { failures.push(format!("{setup}: {}",e.lines().next().unwrap())); }
    }
    assert!(failures.is_empty(), "setter calls: {failures:?}");
}

#[test]
fn accessor_lookup_unknown_key_keeps_every_matching_getter() {
    lookup_site(
        r#"
declare function external(x: unknown): unknown;
class Payload { n = 1; }
const payload = new Payload();
const receiver = { get x() { return payload; }, get y() { return external(0); } };
const getter = receiver.__lookupGetter__(process.argv[0]);
const value = getter.call(receiver);
console.log(value.n); // LOOKUP_N
"#,
        false,
    )
    .unwrap();
}

#[test]
fn accessor_lookup_unknown_receiver_keeps_unknown_callable_results() {
    lookup_site(
        r#"
declare function unknown(): any;
class Payload { n = 1; }
const payload = new Payload();
const getter = unknown().__lookupGetter__("x");
getter.call(null, payload);
console.log(payload.n); // LOOKUP_N
"#,
        false,
    )
    .unwrap();
}

#[test]
fn accessor_lookup_key_coercion_keeps_its_real_effects() {
    lookup_site(
        r#"
declare function external(x: unknown): void;
class Payload { n = 1; }
const payload = new Payload();
const key = { toString() { external(payload); return "x"; } };
const receiver = {};
receiver.__lookupGetter__(key);
console.log(payload.n); // LOOKUP_N
"#,
        false,
    )
    .unwrap();
}

#[test]
fn accessor_lookup_opaque_accessor_stays_unproven() {
    lookup_site(
        r#"
declare function unknown(): any;
const receiver = {};
receiver.__defineGetter__("x", unknown());
const getter = receiver.__lookupGetter__("x");
const value = getter.call(receiver);
console.log(value.n); // LOOKUP_N
"#,
        false,
    )
    .unwrap();
}

#[test]
fn accessor_lookup_proxy_descriptor_traps_keep_their_real_effects() {
    lookup_site(r#"
declare function external(x: unknown): void;
class Payload { n = 1; }
const payload = new Payload();
const receiver = new Proxy({}, { getOwnPropertyDescriptor(target, key) { external(payload); return undefined; } });
receiver.__lookupGetter__("x");
console.log(payload.n); // LOOKUP_N
"#, false).unwrap();
}

#[test]
fn accessor_lookup_primitive_key_coercion_keeps_its_real_effects() {
    for receiver in ["1", "true", "'s'"] {
        for method in ["__lookupGetter__", "__lookupSetter__"] {
            lookup_site(
                &format!(
                    r#"
declare function external(x: unknown): void;
class Payload {{ n = 1; }}
const payload = new Payload();
const key = {{ toString() {{ external(payload); return "x"; }} }};
const receiver = {receiver};
receiver.{method}(key);
console.log(payload.n); // LOOKUP_N
"#
                ),
                false,
            )
            .unwrap();
        }
    }
}

#[test]
fn accessor_lookup_unused_query_does_not_run_the_accessor() {
    for setup in [
        "const receiver = { get x() { external(payload); return 0; } };",
        "class Holder { get x() { external(payload); return 0; } } const receiver = new Holder();",
        "const receiver = {}; Object.defineProperty(receiver, 'x', { get: () => external(payload) });",
        "const receiver = {}; receiver.__defineGetter__('x', () => external(payload));",
    ] {
        lookup_site(&format!(r#"
declare function external(x: unknown): void;
class Payload {{ n = 1; }}
const payload = new Payload();
{setup}
receiver.__lookupGetter__("x");
console.log(payload.n); // LOOKUP_N
"#), true).unwrap();
    }
}

#[test]
fn accessor_lookup_returns_setter_functions_from_every_install_form() {
    for setup in [
        "const receiver = { set x(value) { return payload; } };",
        "class Holder { set x(value) { return payload; } } const receiver = new Holder();",
        "class Base { set x(value) { return payload; } } class Holder extends Base {} const receiver = new Holder();",
        "const receiver = {}; Object.defineProperty(receiver, 'x', { set: (value) => payload });",
        "const receiver = {}; receiver.__defineSetter__('x', (value) => payload);",
    ] {
        lookup_site(&format!(r#"
class Payload {{ n = 1; }}
const payload = new Payload();
{setup}
const setter = receiver.__lookupSetter__("x");
const value = setter.call(receiver, 0);
console.log(value.n); // LOOKUP_N
"#), true).unwrap();
    }
}

#[test]
fn accessor_lookup_getter_and_data_shadow_the_inherited_setter() {
    for member in ["x = 0;", "get x() { return 0; }"] {
        lookup_site(
            &format!(
                r#"
declare function external(x: unknown): void;
class Payload {{ n = 1; }}
const payload = new Payload();
class Base {{ set x(value) {{ external(value); }} }}
class Holder extends Base {{ {member} }}
const receiver = new Holder();
const setter = receiver.__lookupSetter__("x");
if (setter) setter.call(receiver, payload);
console.log(payload.n); // LOOKUP_N
"#
            ),
            true,
        )
        .unwrap();
    }
}
