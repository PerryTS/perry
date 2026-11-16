//! A read of a key no receiver holds, on closed fixed literal layouts whose
//! prototype is the unwritten intrinsic `Object.prototype` that lacks the
//! key, gives `undefined` on every receiver: one outcome, as one slot is
//! (`derive::absent_key`). A key some receiver holds and another lacks is
//! two outcomes ("slots differ"); anything that may give the key a value
//! keeps the read unproven.

use super::*;

fn reads(pf: &ProgramFacts, key: &str) -> Vec<Verdict> {
    pf.sites
        .iter()
        .filter(|s| s.key == key && !s.is_call)
        .map(|s| s.verdict.clone())
        .collect()
}

fn proven(pf: &ProgramFacts, key: &str) -> bool {
    let v = reads(pf, key);
    !v.is_empty()
        && v.iter()
            .all(|v| matches!(v, Verdict::T0Mono | Verdict::T0Set))
}

fn open(pf: &ProgramFacts, key: &str) -> bool {
    let v = reads(pf, key);
    !v.is_empty() && v.iter().all(|v| matches!(v, Verdict::No(_)))
}

const SEARCH: &str = r#"
    interface Opts { level?: number; maxDepth?: number }
    function search(options: Opts = {}): number {
        return (options.level ?? 5) * 10 + (options.maxDepth ?? 3);
    }
    "#;

#[test]
fn a_key_no_literal_layout_holds_reads_undefined() {
    let pf = facts(&format!(
        "{SEARCH} console.log(search({{ level: 2 }}), search());"
    ));
    assert!(
        proven(&pf, "maxDepth"),
        "{:?}\n{}",
        reads(&pf, "maxDepth"),
        pf.dump_text()
    );
    // `{ level }` holds it, `{}` does not: two outcomes.
    assert_eq!(
        reads(&pf, "level"),
        vec![Verdict::No("slots differ across the receiver set".into())],
        "{}",
        pf.dump_text()
    );
}

#[test]
fn an_absent_key_on_one_layout_is_proven() {
    let pf = facts(&format!(
        "{SEARCH} console.log(search({{ level: 2 }}), search({{ level: 4 }}));"
    ));
    assert!(
        proven(&pf, "maxDepth"),
        "{:?}\n{}",
        reads(&pf, "maxDepth"),
        pf.dump_text()
    );
    assert!(
        proven(&pf, "level"),
        "{:?}\n{}",
        reads(&pf, "level"),
        pf.dump_text()
    );
}

/// Soundness: each of these may give `maxDepth` a value on the receiver or
/// its chain, so the read stays unproven.
#[test]
fn a_key_something_may_write_stays_open() {
    for (what, extra) in [
        ("a store after birth", "const o: Opts = { level: 2 }; o.maxDepth = 1; console.log(search(o));"),
        ("a computed store", "const o: any = { level: 2 }; o[String(Date.now() % 2)] = 1; console.log(search(o));"),
        ("an escape", "import \"unseen-host\"; const o: Opts = { level: 2 }; (globalThis as any).out = o; console.log(search(o));"),
        ("a store into ⊤ that may be the receiver", "import \"unseen-host\"; const o: Opts = { level: 2 }; (globalThis as any).out = o; const t: any = (globalThis as any).someHost; t.maxDepth = 3; console.log(search(o));"),
    ] {
        let pf = facts(&format!("{SEARCH} console.log(search({{ level: 2 }})); {extra}"));
        assert!(open(&pf, "maxDepth"), "{what}: {:?}\n{}", reads(&pf, "maxDepth"), pf.dump_text());
    }
}

/// DEC 250: `Object.prototype.maxDepth = 7` is an add to an add-locked
/// prototype: the runtime throws a TypeError at the store and writes
/// nothing (`members::store_admitted`), so the read stays `undefined`.
#[test]
fn an_object_prototype_add_is_refused_so_the_read_stays_proven() {
    let pf = facts(&format!(
        "{SEARCH} console.log(search({{ level: 2 }})); try {{ (Object.prototype as any).maxDepth = 7; }} catch {{}} console.log(search());"
    ));
    assert!(proven(&pf, "maxDepth"), "{:?}\n{}", reads(&pf, "maxDepth"), pf.dump_text());
}

/// A store into ⊤ reaches only what reached unknown code, and an add to
/// `Object.prototype` is refused at the store (DEC 250,
/// `perry_abi::lock::refuses`): the literal that never escaped and its
/// chain cannot gain `maxDepth`, so the read stays proven.
#[test]
fn a_store_into_top_with_no_path_to_the_receiver_is_proven() {
    let pf = facts(&format!(
        "{SEARCH} console.log(search({{ level: 2 }})); import \"unseen-host\"; const t: any = (globalThis as any).someHost; t.maxDepth = 3;"
    ));
    assert!(proven(&pf, "maxDepth"), "{:?}\n{}", reads(&pf, "maxDepth"), pf.dump_text());
}

/// An intrinsic member of `Object.prototype` is no absent key.
#[test]
fn an_object_prototype_member_is_not_absent() {
    let pf = facts(
        r#"
        const o = { level: 2 };
        const f = (o as any).hasOwnProperty;
        console.log(typeof f, (o as any).constructor === Object);
        "#,
    );
    assert!(
        open(&pf, "hasOwnProperty"),
        "{:?}\n{}",
        reads(&pf, "hasOwnProperty"),
        pf.dump_text()
    );
    assert!(
        open(&pf, "constructor"),
        "{:?}\n{}",
        reads(&pf, "constructor"),
        pf.dump_text()
    );
}

/// A literal with its own prototype is not a plain chain.
#[test]
fn a_literal_with_a_prototype_is_not_absent() {
    let pf = facts(
        r#"
        const p = { maxDepth: 9 };
        const o: any = { __proto__: p, level: 2 };
        console.log(o.maxDepth);
        "#,
    );
    assert!(
        open(&pf, "maxDepth"),
        "{:?}\n{}",
        reads(&pf, "maxDepth"),
        pf.dump_text()
    );
}
