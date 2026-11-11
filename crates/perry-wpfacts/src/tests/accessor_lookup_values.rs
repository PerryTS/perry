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

/// B.2.2.4/5 under an unknown key: a modeled receiver's chain answers
/// `undefined`, its program accessors, or a realm accessor function
/// (`Object.prototype.__proto__`'s): no value of the program beyond those.
#[test]
fn accessor_lookup_unknown_key_on_a_known_chain_returns_no_unknown_value() {
    let mut failures = Vec::new();
    for method in ["__lookupGetter__", "__lookupSetter__"] {
        let src = format!(
            r#"
class Payload {{ n = 1; }}
const payload = new Payload();
const receiver = {{ get x() {{ return 1; }}, set x(v) {{}} }};
const found: any = receiver.{method}(process.argv[0]);
if (found) found.held = payload;
console.log(payload.n); // LOOKUP_N
"#
        );
        if let Err(e) = lookup_site(&src, true) {
            failures.push(format!("{method}: {}", e.lines().next().unwrap()));
        }
    }
    assert!(failures.is_empty(), "any-key lookups: {failures:?}");
}

/// The tssrc shape (debug.ts `assertionCache` / `Debug[key] = ..`): a
/// computed read gives "some Object.prototype method", stored as a member
/// of a namespace object, so its call may be `__lookupGetter__` with an
/// unknown key on that object. Its result adds no unknown value. (Stored
/// under a named key: a store under any key is the changed-holder rule.)
#[test]
fn accessor_lookup_method_summary_call_adds_no_unknown_value() {
    let src = r#"
class Payload { n = 1; }
const payload = new Payload();
const ns = { check(x: unknown) { return payload; } };
const cache: Record<string, unknown> = {};
if (process.argv.length > 9) (ns as any).check = cache[process.argv[1]];
const r = ns.check(process.argv[0]);
console.log(r.n); // LOOKUP_N
"#;
    let at = src.find("// LOOKUP_N").unwrap();
    let line = src[..at].rfind('\n').map_or(0, |p| p + 1) as u32;
    let pf = facts(src);
    let sites: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| !s.is_call && s.key == "n" && (line..at as u32).contains(&s.byte_offset))
        .collect();
    assert!(!sites.is_empty(), "missing LOOKUP_N\n{}", pf.dump_text());
    // The call's result holds the method's own result, the summary's
    // values (the receiver, primitives) and a realm accessor function:
    // no unknown value.
    assert!(
        sites.iter().all(|s| !s.top),
        "LOOKUP_N receiver holds an unknown value\n{}",
        pf.dump_text()
    );
}

/// A plain store through ⊤ is [[Set]] (ECMA-262 10.1.9.2): it makes a data
/// property on whatever ⊤ is and never installs an accessor, so the any-key
/// lookup finds only the program accessors and the realm's own.
#[test]
fn accessor_lookup_unknown_key_after_a_store_through_unknown_is_proven() {
    lookup_site(
        r#"
declare function unknown(): any;
class Payload { n = 1; }
const payload = new Payload();
unknown()[process.argv[1]] = () => 0;
const receiver = { get x() { return 1; } };
const found: any = receiver.__lookupGetter__(process.argv[0]);
if (found) found.held = payload;
console.log(payload.n); // LOOKUP_N
"#,
        true,
    )
    .unwrap();
}

/// Control: the found function may be the realm's `__proto__` setter;
/// called with the payload as receiver it is an unknown call.
#[test]
fn accessor_lookup_unknown_key_found_function_called_on_the_payload_stays_unproven() {
    lookup_site(
        r#"
class Payload { n = 1; }
const payload = new Payload();
const receiver = { get x() { return 1; } };
const found: any = receiver.__lookupSetter__(process.argv[0]);
if (found) found.call(payload, null);
console.log(payload.n); // LOOKUP_N
"#,
        false,
    )
    .unwrap();
}
