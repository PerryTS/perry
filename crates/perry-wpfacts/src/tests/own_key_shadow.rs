//! DECISIONS 234: an own definite data key shadows the prototype chain on
//! read. A store into ⊤ under a computed key may write every key of
//! `Object.prototype`; a key every object of the receiver holds as an own
//! data property from birth never reaches it, so the read is the own
//! field's value (plus the chain's program values, a superset), not the
//! builtin prototype's unknown member. Anything that may remove or redefine
//! the own key revokes it: `delete` (named, computed), `defineProperty`
//! (through ⊤ too, which may be a builtin prototype whose setter intercepts
//! the store that makes the key), and a `delete` through ⊤ on an object
//! that escaped (⊤ denotes escaped objects only, DECISIONS 150).

use super::*;

/// `h` is a value from unmodeled code; `h[name] = …` may write any key of
/// `Object.prototype` (node: `hostFn` returns `Object.prototype`).
fn size_verdict(body: &str) -> Verdict {
    let src = format!(
        r#"
        declare const name: string;
        const h: any = (globalThis as any).hostFn();
        h[name] = 1;
        class Svc {{ size = 3; }}
        {body}
        "#
    );
    site(&facts(&src), "size").verdict.clone()
}

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

const P: &str = r#"
    class P {
        svc: Svc;
        constructor(s: Svc) { this.svc = s; }
        total() { return this.svc.size; }
    }
"#;

#[test]
fn a_constructor_store_shadows_a_top_written_prototype() {
    let v = size_verdict(&format!("{P} console.log(new P(new Svc()).total());"));
    assert!(proven(&v), "{v:?}");
}

#[test]
fn a_field_initializer_shadows_a_top_written_prototype() {
    let v = size_verdict(
        "class F { svc = new Svc(); total() { return this.svc.size; } } \
         console.log(new F().total());",
    );
    assert!(proven(&v), "{v:?}");
}

#[test]
fn a_subclass_instance_holds_its_parents_definite_key() {
    let v = size_verdict(&format!(
        "{P} class Q extends P {{ k = 1; }} console.log(new Q(new Svc()).total());"
    ));
    assert!(proven(&v), "{v:?}");
}

#[test]
fn a_literal_key_shadows_a_top_written_prototype() {
    let v = size_verdict("const o = { svc: new Svc() }; console.log(o.svc.size);");
    assert!(proven(&v), "{v:?}");
}

#[test]
fn a_deleted_key_reads_the_prototype() {
    let v = size_verdict(&format!(
        "{P} const p: any = new P(new Svc()); delete p.svc; console.log(p.total());"
    ));
    assert!(!proven(&v), "{v:?}");
}

#[test]
fn a_computed_delete_reads_the_prototype() {
    let v = size_verdict(&format!(
        "{P} const p: any = new P(new Svc()); delete p[name]; console.log(p.total());"
    ));
    assert!(!proven(&v), "{v:?}");
}

#[test]
fn a_delete_through_top_keeps_the_key_of_an_object_that_never_escaped() {
    let v = size_verdict(&format!(
        "{P} const p = new P(new Svc()); delete h.svc; console.log(p.total());"
    ));
    assert!(proven(&v), "{v:?}");
}

#[test]
fn a_computed_delete_through_top_keeps_the_key_of_an_object_that_never_escaped() {
    let v = size_verdict(&format!(
        "{P} const p = new P(new Svc()); delete h[name]; console.log(p.total());"
    ));
    assert!(proven(&v), "{v:?}");
}

/// node: `hostFn(p)` hands `p` out and `hostGet()` returns it; the delete
/// through ⊤ removes `p.svc`, so `this.svc` reads `Object.prototype.svc`.
#[test]
fn a_computed_delete_through_top_revokes_the_key_of_an_escaped_object() {
    let v = size_verdict(&format!(
        "{P} const p = new P(new Svc()); (globalThis as any).hostFn(p); \
         delete (globalThis as any).hostGet()[name]; console.log(p.total());"
    ));
    assert!(!proven(&v), "{v:?}");
}

#[test]
fn a_define_property_through_top_revokes_the_key() {
    let v = size_verdict(&format!(
        "{P} Object.defineProperty(h, name, {{ get() {{ return 1; }} }}); \
         console.log(new P(new Svc()).total());"
    ));
    assert!(!proven(&v), "{v:?}");
}

#[test]
fn a_define_property_on_the_receiver_revokes_the_key() {
    let v = size_verdict(&format!(
        "{P} const p: any = new P(new Svc()); \
         Object.defineProperty(p, \"svc\", {{ get() {{ return 1; }} }}); console.log(p.total());"
    ));
    assert!(!proven(&v), "{v:?}");
}

#[test]
fn an_escaped_receiver_reads_the_prototype() {
    let v = size_verdict(&format!(
        "{P} const p = new P(new Svc()); (globalThis as any).hostFn(p); console.log(p.total());"
    ));
    assert!(!proven(&v), "{v:?}");
}

#[test]
fn a_receiver_without_the_key_reads_the_prototype() {
    let v = size_verdict(
        "declare const c: boolean; \
         const o: any = c ? { svc: new Svc() } : { k: 1 }; console.log(o.svc.size);",
    );
    assert!(!proven(&v), "{v:?}");
}
