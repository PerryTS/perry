//! A computed store `a[i] = v` whose key holds only Numbers stores at an
//! index (ToPropertyKey of a Number is a numeric name): it writes the
//! array's elements, never one of its named non-index keys (DECISIONS
//! 217(c) for stores). A key that may be anything else is a computed-key
//! store, which may name any key.

use super::*;

const PROG: &str = r#"
    class C { k: number; constructor() { this.k = 1; } }
    function g(x: any) { x.k = 2; return 0; }
    const a: any[] = [];
    for (let i = 0; i < 2; i++) { a[i] = g; }
    const c = new C();
"#;

/// `a[i] = g` with a Number `i` is an element: `a.fill(c)` calls the
/// builtin `fill`, never `g`, so `C.k` is written only by its constructor.
/// As a named-key store, `g` was a candidate value of `a.fill`, and the call
/// wrote `c.k`.
#[test]
fn number_key_store_writes_an_element() {
    let pf = facts(&format!("{PROG}\n    a.fill(c);\n    console.log(c.k);"));
    let k = field(tmpl(&pf, "C"), "k");
    assert!(k.readonly && k.writers_post == 0, "{}", pf.dump_text());
}

/// A string key from outside the program may be `"fill"`: `a[s] = g` makes
/// `a.fill(c)` call `g`.
#[test]
fn string_key_store_may_name_a_method() {
    let pf = facts(&format!(
        "{PROG}\n    const s: string = process.argv[2];\n    a[s] = g;\n    a.fill(c);\n    console.log(c.k);"
    ));
    let k = field(tmpl(&pf, "C"), "k");
    assert!(!k.readonly && k.writers_post > 0, "{}", pf.dump_text());
}

/// A key that may be a Number or a string stores under any key.
#[test]
fn mixed_key_store_may_name_a_method() {
    let pf = facts(&format!(
        "{PROG}\n    const s: any = process.argv.length > 2 ? \"fill\" : 0;\n    a[s] = g;\n    a.fill(c);\n    console.log(c.k);"
    ));
    let k = field(tmpl(&pf, "C"), "k");
    assert!(!k.readonly && k.writers_post > 0, "{}", pf.dump_text());
}

/// An object key converts through its `toString`: it may name a method.
#[test]
fn object_key_store_may_name_a_method() {
    let pf = facts(&format!(
        "{PROG}\n    const o = {{ toString() {{ return \"fill\"; }} }};\n    (a as any)[o as any] = g;\n    a.fill(c);\n    console.log(c.k);"
    ));
    let k = field(tmpl(&pf, "C"), "k");
    assert!(!k.readonly && k.writers_post > 0, "{}", pf.dump_text());
}

/// A numeric name stored by a Number key reads back by that name.
#[test]
fn number_key_store_reads_back_by_numeric_name() {
    let pf = facts(&format!(
        "{PROG}\n    const j = -1;\n    a[j] = g;\n    (a as any)[\"-1\"](c);\n    console.log(c.k);"
    ));
    let k = field(tmpl(&pf, "C"), "k");
    assert!(!k.readonly && k.writers_post > 0, "{}", pf.dump_text());
}
