//! A function-born array literal's summary birth records the literal's
//! elements as its `Key::Any` field. Replayed, that field is the token's
//! element slot (`field_any`), not its named-key slot: an element is not the
//! value of `slice` or `includes`. A token whose named keys already hold a
//! value (a computed-key store) has no birth for that content and is not
//! born from the summary.

use super::*;

/// The verdicts of the method-call sites of `key`.
fn calls(pf: &ProgramFacts, key: &str) -> Vec<Verdict> {
    pf.sites
        .iter()
        .filter(|s| s.key == key && s.is_call)
        .map(|s| s.verdict.clone())
        .collect()
}

fn native(pf: &ProgramFacts, key: &str) -> bool {
    let v = calls(pf, key);
    !v.is_empty() && v.iter().all(|v| *v == Verdict::NativeMember)
}

fn unproven(pf: &ProgramFacts, key: &str) -> bool {
    let v = calls(pf, key);
    !v.is_empty() && v.iter().all(|v| matches!(v, Verdict::No(_)))
}

/// A literal returned by a function, passed on, and sliced.
#[test]
fn returned_literal_slice_is_native() {
    let pf = facts(
        r#"
        function g(): number[] { return [1]; }
        function o(m: readonly number[]): number[] { return m.slice(); }
        console.log(o(g()).length);
    "#,
    );
    assert!(
        native(&pf, "slice"),
        "{:?}\n{}",
        calls(&pf, "slice"),
        pf.dump_text()
    );
}

/// An empty literal filled by `push`, returned, searched.
#[test]
fn pushed_literal_includes_is_native() {
    let pf = facts(
        r#"
        function legal(): number[] { const a: number[] = []; a.push(1); return a; }
        console.log(legal().includes(1));
    "#,
    );
    assert!(
        native(&pf, "includes"),
        "{:?}\n{}",
        calls(&pf, "includes"),
        pf.dump_text()
    );
}

/// The pushed literal, passed on and sliced.
#[test]
fn pushed_literal_passed_on_slice_is_native() {
    let pf = facts(
        r#"
        function legal(): number[] { const a: number[] = []; a.push(1); return a; }
        function order(moves: readonly number[]): number[] { return moves.slice(); }
        console.log(order(legal()).length);
    "#,
    );
    assert!(
        native(&pf, "slice"),
        "{:?}\n{}",
        calls(&pf, "slice"),
        pf.dump_text()
    );
}

/// The pushed element is a parameter.
#[test]
fn pushed_parameter_includes_is_native() {
    let pf = facts(
        r#"
        function legal(n: number): number[] { const a: number[] = []; a.push(n); return a; }
        console.log(legal(1).includes(1));
    "#,
    );
    assert!(
        native(&pf, "includes"),
        "{:?}\n{}",
        calls(&pf, "includes"),
        pf.dump_text()
    );
}

/// A non-empty literal bound to a local and returned.
#[test]
fn bound_literal_includes_is_native() {
    let pf = facts(
        r#"
        function legal(): number[] { const a: number[] = [1]; return a; }
        console.log(legal().includes(1));
    "#,
    );
    assert!(
        native(&pf, "includes"),
        "{:?}\n{}",
        calls(&pf, "includes"),
        pf.dump_text()
    );
}

/// Soundness: a computed-key store may write `slice` itself, so the
/// array's named keys hold a value and `slice` stays unproven.
#[test]
fn computed_key_store_keeps_slice_unproven() {
    let pf = facts(
        r#"
        function g(k: string): number[] { const a: any = [1]; a[k] = 5; return a; }
        function o(m: readonly number[]): number[] { return m.slice(); }
        console.log(o(g("slice")).length);
    "#,
    );
    assert!(
        unproven(&pf, "slice"),
        "{:?}\n{}",
        calls(&pf, "slice"),
        pf.dump_text()
    );
}
