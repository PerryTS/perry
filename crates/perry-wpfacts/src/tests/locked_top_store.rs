//! DEC 250, a store into ⊤: a ⊤ value may be a built-in prototype, but the
//! runtime refuses a store that replaces an image member of a locked
//! prototype or adds a key to an add-locked one (`Object.prototype`,
//! `Function.prototype`, the iterator prototypes; `constructor` exempt).
//! Such a store throws and writes nothing, so a read that only passes those
//! prototypes stays final. One predicate answers "can a store of this key on
//! this built-in holder succeed" (`members::store_admitted`, on
//! `perry_abi::lock::refuses`) for the chain read, `derive::absent_key` and
//! the written-through-⊤ gate. Where the runtime admits the store
//! (`Array.prototype` named adds, a program prototype, `constructor`) the
//! read stays open.
use super::*;

fn sites(pf: &ProgramFacts, key: &str, call: bool) -> Vec<Verdict> {
    pf.sites
        .iter()
        .filter(|s| s.key == key && s.is_call == call)
        .map(|s| s.verdict.clone())
        .collect()
}

fn proven(v: &[Verdict]) -> bool {
    !v.is_empty() && v.iter().all(|v| matches!(v, Verdict::T0Mono | Verdict::T0Set))
}

fn open(v: &[Verdict]) -> bool {
    !v.is_empty() && v.iter().all(|v| matches!(v, Verdict::No(_)))
}

/// `u` may be `holder` or ⊤ (a global named by the command line).
fn program(holder: &str, store: &str, body: &str) -> String {
    format!(
        r#"
        const u: any = process.argv.length > 9 ? {holder} : (globalThis as any)[process.argv[2]];
        function f(this: any, k: any): any {{ return k; }}
        {store}
        {body}
        "#
    )
}

#[test]
fn a_store_into_top_replacing_an_object_prototype_member_writes_nothing() {
    let pf = facts(&program(
        "Object.prototype",
        "u.hasOwnProperty = f;",
        "const o = { a: 1 }; console.log(o.hasOwnProperty(\"a\"), o.a);",
    ));
    let v = sites(&pf, "hasOwnProperty", true);
    assert!(proven(&v), "{v:?}\n{}", pf.dump_text());
}

#[test]
fn a_store_into_top_adding_to_object_prototype_writes_nothing() {
    let pf = facts(&program(
        "Object.prototype",
        "u.isX = 1;",
        "const o = { a: 1 }; console.log(o.isX, o.a);",
    ));
    let v = sites(&pf, "isX", false);
    assert!(proven(&v), "{v:?}\n{}", pf.dump_text());
}

/// Control (OVMB9, three.js `Cache.files[url]`): an unknown key on a plain
/// object may read an `Object.prototype` method. Past its own keys that
/// function's chain is `Function.prototype` then `Object.prototype` (both
/// refuse the add), but the function object itself is no locked prototype:
/// a store into ⊤ may add `userData` to it, so the read stays open.
#[test]
fn an_own_key_of_a_builtin_method_stays_open() {
    let pf = facts(&program(
        "Object.prototype",
        "u.userData = { k: 1 };",
        "const files: any = {}; const c = files[process.argv[3]]; \
         if (c !== undefined) console.log(c.userData);",
    ));
    let v = sites(&pf, "userData", false);
    assert!(open(&v), "{v:?}\n{}", pf.dump_text());
}

/// Controls: the runtime admits these stores, the reads stay open.
#[test]
fn an_admitted_store_into_top_keeps_the_read_open() {
    // A named add on Array.prototype is admitted.
    let pf = facts(&program(
        "Array.prototype",
        "u.extra = f;",
        "const xs: any = [1, 2]; console.log(xs.extra);",
    ));
    let v = sites(&pf, "extra", false);
    assert!(open(&v), "array add {v:?}\n{}", pf.dump_text());
    // `constructor` is exempt from the lock.
    let pf = facts(&program(
        "Object.prototype",
        "u.constructor = f;",
        "const o: any = { a: 1 }; console.log(o.constructor, o.a);",
    ));
    let v = sites(&pf, "constructor", false);
    assert!(open(&v), "constructor {v:?}\n{}", pf.dump_text());
    // A program prototype that reached unknown code takes the add.
    let pf = facts(&program(
        "Object.prototype",
        "class K { a = 1; } (globalThis as any).K = K; u.isY = 1;",
        "const k: any = new K(); console.log(k.isY, k.a);",
    ));
    let v = sites(&pf, "isY", false);
    assert!(open(&v), "program proto {v:?}\n{}", pf.dump_text());
    // The key comes from the command line: unknown code may name any key.
    let pf = facts(&program(
        "Object.prototype",
        "const p: any = { a: 1 }; (globalThis as any).keep = p; u[process.argv[3]] = 1;",
        "console.log(p.isZ, p.a);",
    ));
    let v = sites(&pf, "isZ", false);
    assert!(open(&v), "escaped literal {v:?}\n{}", pf.dump_text());
    // Unknown code: `new Function` may write the key onto anything it holds.
    let pf = facts(&program(
        "Object.prototype",
        "const q: any = { a: 1 }; new Function(process.argv[2])(q);",
        "console.log(q.isW, q.a);",
    ));
    let v = sites(&pf, "isW", false);
    assert!(open(&v), "new Function {v:?}\n{}", pf.dump_text());
}
