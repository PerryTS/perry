//! A computed key carries its string constants (DESIGN SEEDKV): a store
//! `t[key] = v` whose key holds only constant strings is a named store
//! under each of them, so a store into a runtime object (`Object`, its
//! prototype's methods) through such a key writes those keys alone and
//! gives it no prototype. A key that may be any string (a written field, an
//! escaped literal, a concatenation, a symbol) keeps the
//! any-key store, and with it the prototype a `__proto__` key may give.
//!
//! The witness: `box` is the stored value. Given to a runtime object as its
//! prototype, its members escape (E6), so `box.svc` and `box.svc.size` lose
//! their facts.

use super::*;

/// tsc's `setCachedIterationTypes` shape: the receiver may be `Object` or
/// one of its prototype's methods (an any-key read of a plain literal), the
/// key a resolver field a literal wrote once. `{extra}` is the control.
fn src(extra: &str) -> String {
    format!(
        r#"
        declare const w: boolean;
        const plain: any = {{ x: 1 }};
        function readAny(o: any, k: string) {{ return o[k]; }}
        const R1: any = {{ cacheKey: "iterA" }}, R2: any = {{ cacheKey: "iterB" }};
        function setCached(t: any, key: string, v: any) {{ return t[key] = v; }}
        function getCached(t: any, key: string) {{ return t[key]; }}
        function via(t: any, r: {{ cacheKey: string }}, v: any) {{ return setCached(t, r.cacheKey, v); }}
        const recv = readAny(plain, String(Date.now()));
        class Svc {{ size = 3; }}
        const box: any = {{ svc: new Svc() }};
        via(recv, R1, box); via(recv, R2, box);
        setCached(recv, Math.random() > 0.5 ? "iterC" : "iterD", box);
        getCached(recv, R1.cacheKey);
        {extra}
        console.log(Object.keys({{ z: 1 }}).length);
        console.log(box.svc.size);
        "#
    )
}

fn verdict_of(pf: &ProgramFacts, key: &str) -> Verdict {
    pf.sites
        .iter()
        .find(|s| s.key == key)
        .unwrap_or_else(|| panic!("site .{key} in\n{}", pf.dump_text()))
        .verdict
        .clone()
}

/// No prototype was given to a runtime object: `box`'s members keep their
/// facts.
fn no_give(extra: &str) -> bool {
    let pf = facts(&src(extra));
    ["svc", "size"]
        .iter()
        .all(|k| !matches!(verdict_of(&pf, k), Verdict::No(_)))
}

#[test]
fn constant_keys_through_a_resolver_field_are_named_stores() {
    assert!(no_give(""));
}

#[test]
fn a_constant_index_string_is_an_element_store() {
    // `"0"` is ToPropertyKey of 0: the numeric properties, never `__proto__`.
    assert!(no_give(r#"setCached(recv, "0", box);"#));
}

#[test]
fn true_null_and_undefined_keys_are_named_stores() {
    assert!(no_give(
        "setCached(recv, w ? true : null, box); setCached(recv, undefined as any, box);"
    ));
}

#[test]
fn a_concatenation_whose_shape_excludes_proto_gives_nothing() {
    // (c4b) any string, but the store's key shape `iter*` never names
    // `__proto__` (a shape is the store site's: through `setCached`'s
    // parameter the key is any string of any shape).
    assert!(no_give(r#"recv["iter" + String(Date.now())] = box;"#));
}

#[test]
fn a_proto_constant_gives_the_prototype() {
    // (c1) the give stays where the program spells `__proto__`.
    assert!(!no_give(r#"via(recv, { cacheKey: "__proto__" }, box);"#));
}

#[test]
fn a_written_field_is_any_string() {
    // (c2)
    assert!(!no_give("R1.cacheKey = String(Date.now());"));
}

#[test]
fn an_any_key_string_store_into_the_resolver_is_any_string() {
    // (c2b) the store may write `cacheKey`.
    assert!(!no_give(
        "(R1 as any)[String(Date.now())] = String(Math.random());"
    ));
}

#[test]
fn an_escaped_resolver_is_any_string() {
    // (c3) unknown code holds `R1` and may write its field.
    assert!(!no_give("(globalThis as any).hostFn(R1);"));
}

#[test]
fn a_concatenation_whose_shape_admits_proto_gives() {
    // (c4) any string of shape `__pro*`.
    assert!(!no_give(r#"recv["__pro" + String(Date.now())] = box;"#));
}

#[test]
fn nine_constants_stay_names() {
    // (c5) nine constants joined in one return node: exact names (the set
    // is exact past eight), none of them `__proto__`.
    assert!(no_give(
        r#"function pick(i: number): string {
            if (i == 1) return "k1"; if (i == 2) return "k2"; if (i == 3) return "k3";
            if (i == 4) return "k4"; if (i == 5) return "k5"; if (i == 6) return "k6";
            if (i == 7) return "k7"; if (i == 8) return "k8"; return "k9";
        }
        setCached(recv, pick(Date.now()), box);"#
    ));
}

#[test]
fn eight_constants_stay_names() {
    // (c5) the cap itself: eight constants, none `__proto__`.
    assert!(no_give(
        r#"function pick(i: number): string {
            if (i == 1) return "k1"; if (i == 2) return "k2"; if (i == 3) return "k3";
            if (i == 4) return "k4"; if (i == 5) return "k5"; if (i == 6) return "k6";
            if (i == 7) return "k7"; return "k8";
        }
        setCached(recv, pick(Date.now()), box);"#
    ));
}
