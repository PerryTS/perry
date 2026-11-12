//! `Promise.all([e0, …, en-1])` over an array literal (ECMA-262 27.2.4.1.2,
//! 27.2.4.1.3): the literal is fresh and only the iteration holds it, and
//! %ArrayIteratorPrototype% is locked, so item `i` is `ei`; the resolve
//! element function made for item `i` writes index `i` of the result Array
//! only. The result is index-exact (`Model::index_slot`): `results[1]`
//! under the constant key 1 reads what item 1 fulfils with, while nothing
//! stored into the result, moved its elements or handed it to unknown code.
//! (three.js GLTFLoader `loadNode`: `Promise.all([nodePending,
//! Promise.all(childPending), skeletonPending]).then(results => results[1])`.)

use super::*;

/// The sink takes `s` only through a wrong position read.
fn verdict(tail: &str) -> Verdict {
    let src = format!(
        r#"
        class Svc {{ size = 3; }}
        function g(a: any) {{ return a.size; }}
        const s = new Svc();
        const unknown: any = (globalThis as any)[process.argv[2]];
        const take = (x: any) => {{ unknown.take(x); }};
        (function () {{ {tail} }})();
        console.log(g(s));
        "#
    );
    site(&facts(&src), "size").verdict.clone()
}

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

/// node: `r[1]` is what item 1 fulfils with, never item 0 `s`.
#[test]
fn constant_index_reads_its_item() {
    let v = verdict(
        "Promise.all([Promise.resolve(s), Promise.resolve({ b: 2 })]) \
         .then((r: any) => { take(r[1]); });",
    );
    assert!(proven(&v), "{v:?}");
}

/// node: a third position and a plain (non-promise) item.
#[test]
fn third_index_reads_its_item() {
    let v = verdict(
        "Promise.all([s, Promise.resolve(s), { b: 2 }]) \
         .then((r: any) => { take(r[2]); });",
    );
    assert!(proven(&v), "{v:?}");
}

/// Control: the result's index 0 is `s`.
#[test]
fn own_index_holds_its_item() {
    let v = verdict(
        "Promise.all([Promise.resolve(s), Promise.resolve({ b: 2 })]) \
         .then((r: any) => { take(r[0]); });",
    );
    assert!(!proven(&v), "{v:?}");
}

/// Control: a key from `process.argv` may be 0.
#[test]
fn unknown_index_reads_all() {
    let v = verdict(
        "Promise.all([Promise.resolve(s), Promise.resolve({ b: 2 })]) \
         .then((r: any) => { take(r[process.argv[2]]); });",
    );
    assert!(!proven(&v), "{v:?}");
}

/// Control: a key that may be a string or a number.
#[test]
fn string_or_number_key_reads_all() {
    let v = verdict(
        "const k: any = process.argv.length > 2 ? '0' : 1; \
         Promise.all([Promise.resolve(s), Promise.resolve({ b: 2 })]) \
         .then((r: any) => { take(r[k]); });",
    );
    assert!(!proven(&v), "{v:?}");
}

/// Control: a write into the result may move `s` to index 1.
#[test]
fn written_result_merges() {
    let v = verdict(
        "Promise.all([Promise.resolve(s), Promise.resolve({ b: 2 })]) \
         .then((r: any) => { r[1] = r[0]; take(r[1]); });",
    );
    assert!(!proven(&v), "{v:?}");
}

/// Control: `reverse` moves index 0 to index 1.
#[test]
fn reversed_result_merges() {
    let v = verdict(
        "Promise.all([Promise.resolve(s), Promise.resolve({ b: 2 })]) \
         .then((r: any) => { r.reverse(); take(r[1]); });",
    );
    assert!(!proven(&v), "{v:?}");
}

/// Control: a result handed to unknown code may hold anything anywhere.
#[test]
fn escaped_result_stays_open() {
    let v = verdict(
        "Promise.all([Promise.resolve(s), Promise.resolve({ b: 2 })]) \
         .then((r: any) => { new Function(process.argv[2])(r); take(r[1]); });",
    );
    assert!(!proven(&v), "{v:?}");
}

/// Control: an array that is not a literal argument stays merged.
#[test]
fn non_literal_argument_stays_merged() {
    let v = verdict(
        "const ps: any[] = [Promise.resolve(s), Promise.resolve({ b: 2 })]; \
         Promise.all(ps).then((r: any) => { take(r[1]); });",
    );
    assert!(!proven(&v), "{v:?}");
}

/// Control: an item from unknown code that was handed `s` may fulfil with it.
#[test]
fn unknown_item_stays_open() {
    let v = verdict(
        "const p: any = new Function(process.argv[2])(s); \
         Promise.all([Promise.resolve(s), p]).then((r: any) => { take(r[1]); });",
    );
    assert!(!proven(&v), "{v:?}");
}

/// Control: a `JSON.parse(process.argv[2])` key may be 0 (or any key).
#[test]
fn json_key_reads_all() {
    let v = verdict(
        "const k: any = JSON.parse(process.argv[2]); \
         Promise.all([Promise.resolve(s), Promise.resolve({ b: 2 })]) \
         .then((r: any) => { take(r[k]); });",
    );
    assert!(!proven(&v), "{v:?}");
}
