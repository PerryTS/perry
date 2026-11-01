//! `Object.setPrototypeOf(x, p)` writes no own key of any object: it changes
//! only `x`'s [[Prototype]]. When `x` may be a runtime object (⊤ may be
//! `Array.prototype`; `Object.prototype` throws, an immutable prototype), its
//! lookups may now reach the keys of `p`'s chain past it, and only those:
//! each is a written key of the runtime objects. A prototype whose keys are
//! not known (⊤, a computed store into it) writes every key, as before.

use super::*;

/// `h` is a value from unmodeled code (node: `hostFn` returns
/// `Array.prototype`), given prototype `{proto}`. `a[0]` is an element read
/// on an array, which consults the builtin prototypes under its index.
fn verdict(proto: &str, read: &str) -> Verdict {
    let src = format!(
        r#"
        const h: any = (globalThis as any).hostFn();
        {proto}
        Object.setPrototypeOf(h, P);
        class Svc {{ size = 3; }}
        const a: any = [new Svc()];
        console.log(a[0].size);
        console.log(a.greet === undefined ? 0 : 1);
        "#
    );
    site(&facts(&src), read).verdict.clone()
}

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

const LIT: &str = "const P = { greet() { return 1; } };";

#[test]
fn a_given_prototype_writes_only_its_keys() {
    let v = verdict(LIT, "size");
    assert!(proven(&v), "{v:?}");
}

#[test]
fn a_key_of_the_given_prototype_is_written() {
    // node: `a.greet` is P.greet once `Array.prototype`'s chain holds P.
    let v = verdict(LIT, "greet");
    assert!(!proven(&v), "{v:?}");
}

#[test]
fn a_key_stored_into_the_given_prototype_later_is_written() {
    let v = verdict("const P: any = { k: 1 }; P.greet = () => 1;", "greet");
    assert!(!proven(&v), "{v:?}");
}

#[test]
fn an_unknown_prototype_writes_every_key() {
    let v = verdict("const P: any = (globalThis as any).hostProto();", "size");
    assert!(!proven(&v), "{v:?}");
}

#[test]
fn a_computed_store_into_the_given_prototype_writes_every_key() {
    let v = verdict(
        "declare const name: string; const P: any = { k: 1 }; P[name] = 2;",
        "size",
    );
    assert!(!proven(&v), "{v:?}");
}

/// debug.ts `attachFlowNodeDebugInfo`: an `Object.create` prototype whose
/// keys are all defined under constant names holds exactly those keys.
const CREATED: &str = "const P: any = Object.create(Object.prototype);
    Object.defineProperties(P, { greet: { get() { return 1; } }, tag: { value: 2 } });";

#[test]
fn a_created_prototype_with_defined_keys_writes_only_its_keys() {
    let v = verdict(CREATED, "size");
    assert!(proven(&v), "{v:?}");
}

#[test]
fn a_defined_key_of_a_created_prototype_is_written() {
    // node: `a.greet` is the getter of P once the chain of `Array.prototype` holds P.
    let v = verdict(CREATED, "greet");
    assert!(!proven(&v), "{v:?}");
}

#[test]
fn a_created_prototype_defined_under_a_computed_key_writes_every_key() {
    let v = verdict(
        "declare const name: string; const P: any = Object.create(Object.prototype);
         Object.defineProperty(P, name, { value: 1 });",
        "size",
    );
    assert!(!proven(&v), "{v:?}");
}

#[test]
fn a_created_prototype_with_a_computed_accessor_writes_every_key() {
    // No getter value reaches the define: the key is still unknown.
    let v = verdict(
        "declare const name: string; const P: any = Object.create(Object.prototype);
         Object.defineProperty(P, name, { get: undefined });",
        "size",
    );
    assert!(!proven(&v), "{v:?}");
}

#[test]
fn a_created_prototype_from_unknown_descriptors_writes_every_key() {
    let v = verdict(
        "const P: any = Object.create(Object.prototype, (globalThis as any).hostDescs());",
        "size",
    );
    assert!(!proven(&v), "{v:?}");
}
