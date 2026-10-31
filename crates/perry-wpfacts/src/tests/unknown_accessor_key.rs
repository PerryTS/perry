//! An accessor whose getter or setter is an unknown function value defines
//! the one key it is installed under. On a value from unmodeled code (⊤)
//! that may write that key onto `Object.prototype`, and no other: a key
//! some program object only gains late (absent at birth) still reads its
//! own stores or `undefined`, not the opaque member of every key.

use super::*;

/// `l.cache` is not a birth key of `Holder`: absent, the read consults
/// `Object.prototype`. `h` is from unmodeled code.
fn kind_verdict(define: &str, read: &str) -> Verdict {
    let src = format!(
        r#"
        declare const c: boolean;
        declare const name: string;
        const h: any = (globalThis as any).hostFn();
        const g: any = (globalThis as any).hostGet;
        {define}
        class Node {{ kind: number; constructor(k: number) {{ this.kind = k; }} }}
        class Holder {{ a = 1; }}
        const l: any = new Holder();
        if (c) {{ l.cache = new Node(1); l.k = new Node(2); }}
        const n = l.{read};
        if (n) console.log(n.kind);
        "#
    );
    site(&facts(&src), "kind").verdict.clone()
}

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

#[test]
fn no_top_store_reads_the_late_key() {
    // Control: without the accessor the late key's read is its stores.
    let v = kind_verdict("", "cache");
    assert!(proven(&v), "{v:?}");
}

#[test]
fn an_unknown_getter_under_a_named_key_writes_that_key_only() {
    let v = kind_verdict(r#"Object.defineProperty(h, "k", { get: g });"#, "cache");
    assert!(proven(&v), "{v:?}");
}

#[test]
fn an_unknown_setter_under_a_named_key_writes_that_key_only() {
    let v = kind_verdict(r#"Object.defineProperty(h, "k", { set: g });"#, "cache");
    assert!(proven(&v), "{v:?}");
}

#[test]
fn an_unknown_getter_writes_its_own_key() {
    // `Object.prototype.k` may now be the accessor: `l.k` reads ⊤.
    let v = kind_verdict(r#"Object.defineProperty(h, "k", { get: g });"#, "k");
    assert!(!proven(&v), "{v:?}");
}

#[test]
fn an_unknown_getter_under_a_computed_key_writes_every_key() {
    let v = kind_verdict("Object.defineProperty(h, name, { get: g });", "cache");
    assert!(!proven(&v), "{v:?}");
}

#[test]
fn an_unknown_getter_on_a_program_object_reads_top_under_its_key() {
    // The holder is the program's own object: its accessor key reads ⊤
    // (the getter is unknown code), and the object escapes to it.
    let src = r#"
        const g: any = (globalThis as any).hostGet;
        class Node { kind: number; constructor(k: number) { this.kind = k; } }
        const o: any = { k: new Node(1) };
        Object.defineProperty(o, "k", { get: g });
        console.log(o.k.kind);
    "#;
    let v = site(&facts(src), "kind").verdict.clone();
    assert!(!proven(&v), "{v:?}");
}
