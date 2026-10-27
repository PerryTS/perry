//! An Array's named non-index keys are its own keys: its elements (what its
//! construction, `push` and the like put at an index) are read under an
//! index or by iteration only. A store under a computed key may name any
//! key, so the named keys read what such a store stored, and what unknown
//! code may write once the array escaped.

use super::*;

fn recv(pf: &ProgramFacts, key: &str) -> Vec<String> {
    pf.sites
        .iter()
        .filter(|s| s.key == key)
        .flat_map(|s| s.receivers.iter().cloned())
        .collect()
}

fn top(pf: &ProgramFacts, key: &str) -> bool {
    pf.sites.iter().filter(|s| s.key == key).any(|s| s.top)
}

/// A named non-index key of an array reads none of its elements.
#[test]
fn a_named_key_of_an_array_reads_none_of_its_elements() {
    let pf = facts(
        r#"
        class A { a = 1; q = 2; }
        const xs: any[] = [new A(), new A()];
        xs.push(new A());
        const n: any = xs.next;
        if (n) console.log(n.q);
        for (const x of xs) console.log(x.a);
        "#,
    );
    assert!(!recv(&pf, "q").iter().any(|r| r.ends_with(":A")), "{}", pf.dump_text());
    assert!(recv(&pf, "a").iter().any(|r| r.ends_with(":A")), "{}", pf.dump_text());
}

/// The iteration protocol of an array holding a builtin function value: the
/// iterator's `next` and `[Symbol.iterator]` are no element of it, so no
/// element is called with the iterator as its receiver and the elements stay
/// the program's (fast-check `details.fn`, awfy `Bounce`).
#[test]
fn iterating_an_array_of_builtin_functions_calls_none_of_them() {
    let pf = facts(
        r#"
        class A { a = 1; }
        const xs: any[] = [new A(), Math.max];
        for (const x of xs) console.log(x.a);
        "#,
    );
    assert!(tmpl(&pf, "A").escape.is_none(), "{}", pf.dump_text());
}

/// Soundness: a store under a computed key may name any key, an index or
/// not: a named key reads it, an element read as well.
#[test]
fn a_computed_key_store_reaches_the_named_keys() {
    let pf = facts(
        r#"
        class A { a = 1; }
        class C { c = 1; q = 2; }
        declare const k: string;
        const xs: any[] = [new A()];
        xs[k] = new C();
        const v: any = xs.foo;
        console.log(v.q);
        console.log(xs[0].c);
        "#,
    );
    let q = recv(&pf, "q");
    assert!(q.iter().any(|r| r.ends_with(":C")), "{}", pf.dump_text());
    assert!(!q.iter().any(|r| r.ends_with(":A")), "{}", pf.dump_text());
    assert!(recv(&pf, "c").iter().any(|r| r.ends_with(":C")), "{}", pf.dump_text());
}

/// Soundness: a key that may be ToString of a Number is an element key.
#[test]
fn a_numeric_key_reads_the_elements() {
    let pf = facts(
        r#"
        class A { a = 1; q = 2; }
        const xs: any[] = [new A()];
        const v: any = (xs as any)["NaN"];
        if (v) console.log(v.q);
        "#,
    );
    assert!(recv(&pf, "q").iter().any(|r| r.ends_with(":A")), "{}", pf.dump_text());
}

/// Soundness: an escaped array's named keys hold what unknown code wrote.
#[test]
fn an_escaped_arrays_named_keys_read_top() {
    let pf = facts(
        r#"
        class A { a = 1; q = 2; }
        declare function ext(x: any): void;
        const xs: any[] = [new A()];
        ext(xs);
        const v: any = xs.foo;
        if (v) console.log(v.q);
        "#,
    );
    assert!(top(&pf, "q"), "{}", pf.dump_text());
}

