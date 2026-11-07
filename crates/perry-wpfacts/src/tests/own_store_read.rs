//! OWN2 (`gen/own_store.rs`): a computed read `o[k]` after the store
//! `o[k] = v` of the same bindings, in a body where nothing but those keyed
//! accesses touches `o`, reads `o`'s own properties only: never the
//! prototype chain's `Object.prototype` members, so the value stored through
//! it never lands on a built-in (three.js `cloneUniforms`).

use super::*;

fn calls(pf: &ProgramFacts, key: &str) -> Vec<Verdict> {
    pf.sites
        .iter()
        .filter(|s| s.key == key && s.is_call)
        .map(|s| s.verdict.clone())
        .collect()
}

fn proven(pf: &ProgramFacts, key: &str) -> bool {
    let v = calls(pf, key);
    !v.is_empty() && v.iter().all(|v| !matches!(v, Verdict::No(_)))
}

fn unproven(pf: &ProgramFacts, key: &str) -> bool {
    let v = calls(pf, key);
    !v.is_empty() && v.iter().any(|v| matches!(v, Verdict::No(_)))
}

/// `cloneUniforms` reduced: `body` is the outer loop body over `u`, `pre`
/// runs before the program. The uniforms come from a parameter, so the keys
/// are any string (`constructor` included); `size()` is called only on the
/// `V` stored through `dst[u]`.
fn clone(pre: &str, body: &str) -> ProgramFacts {
    facts(&format!(
        r#"
        "use strict";
        {pre}
        class V {{ n = 1; size(): number {{ return this.n; }} }}
        function clone(src: any): any {{
            const dst: any = {{}};
            for (const u in src) {{
                {body}
            }}
            return dst;
        }}
        function make(names: string[]): any {{
            const o: any = {{}};
            for (const n of names) o[n] = {{ p: 1 }};
            return o;
        }}
        const out = clone(make(["a", "constructor", "toString"]));
        console.log(out.a.p[0].size());
        "#
    ))
}

const STORE: &str = "dst[u] = {};";
const INNER: &str = r#"for (const p in src[u]) { const arr = [new V()]; dst[u][p] = arr; }"#;

// ---- site test: unproven on the base, proven with the rule ----

#[test]
fn read_after_store_reads_own_only() {
    let pf = clone("", &format!("{STORE} {INNER}"));
    assert!(proven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

// ---- controls: the read stays ordinary ----

#[test]
fn delete_between_keeps_chain() {
    let pf = clone("", &format!("{STORE} delete dst[u]; {INNER}"));
    assert!(unproven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

#[test]
fn method_call_on_object_keeps_chain() {
    let pf = clone("", &format!("{STORE} if (u === \"z\") dst[u](); {INNER}"));
    assert!(unproven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

#[test]
fn other_key_binding_keeps_chain() {
    let pf = clone(
        "",
        &format!("const w = u + \"x\"; {STORE} for (const p in src[u]) {{ dst[w][p] = [new V()]; dst[u][p] = 1; }}"),
    );
    assert!(unproven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

#[test]
fn read_before_store_keeps_chain() {
    let pf = clone("", &format!("{INNER} {STORE}"));
    assert!(unproven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

#[test]
fn conditional_store_keeps_chain() {
    let pf = clone("", &format!("if (u !== \"q\") {{ {STORE} }} {INNER}"));
    assert!(unproven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

#[test]
fn object_passed_keeps_chain() {
    let pf = clone(
        "function leak(o: any): void {}",
        &format!("{STORE} leak(dst); {INNER}"),
    );
    assert!(unproven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

#[test]
fn closure_capture_keeps_chain() {
    let pf = clone("", &format!("{STORE} const g = () => dst; g(); {INNER}"));
    assert!(unproven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

#[test]
fn prototype_accessor_keeps_chain() {
    let pf = clone(
        r#"Object.defineProperty(Object.prototype, "constructor", {
            set(v: any) { Object.defineProperty(this, "constructor", { value: v }); },
            get() { return 1; }, configurable: true });"#,
        &format!("{STORE} {INNER}"),
    );
    assert!(unproven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

#[test]
fn class_getter_off_chain_reads_own_only() {
    // Same rule, a getter on a program prototype: not reachable from a
    // literal's chain, so the read stays own (a control the other way).
    let pf = clone(
        r#"class Q { get zz(): number { return 1; } } new Q().zz;"#,
        &format!("{STORE} {INNER}"),
    );
    assert!(proven(&pf, "size"), "{:?}", calls(&pf, "size"));
}

#[test]
fn annexb_prototype_setter_keeps_chain() {
    // `Object.prototype.__defineSetter__` under a key the for-in yields:
    // the store runs the setter with `this` = dst, which deletes the key.
    let pf = clone(
        r#"(Object.prototype as any).__defineSetter__("constructor", function (this: any, v: any) {
            delete this.constructor; });"#,
        &format!("{STORE} {INNER}"),
    );
    assert!(unproven(&pf, "size"), "{:?}", calls(&pf, "size"));
}
