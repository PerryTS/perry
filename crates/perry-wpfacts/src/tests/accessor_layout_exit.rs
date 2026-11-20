//! An accessor defined on a key a literal's layout holds (a key a program
//! store writes, an optional slot) leaves the layout: the runtime gives the
//! object its overflow and class id 0, so no field claim of the literal
//! survives it. A new key the layout does not hold stays an accessor key on
//! the layout (`accessor_keys`). SOUND3/SOUND5: test_issue_450_defineProperty_accessor.

use super::*;

fn backing(src: &str) -> Vec<Verdict> {
    let pf = facts(src);
    pf.sites
        .iter()
        .filter(|s| s.key == "_backing")
        .map(|s| s.verdict.clone())
        .collect()
}

#[test]
fn an_accessor_on_a_stored_key_leaves_the_layout() {
    for (name, src) in [
        (
            "store after the definition",
            r#"const a: any = { _backing: 5 };
Object.defineProperty(a, "value", { get() { return this._backing * 2; }, set(v: number) { this._backing = v; }, configurable: true });
console.log(a.value, a._backing);
a.value = 21;
console.log(a._backing);"#,
        ),
        (
            "store before the definition",
            r#"const a: any = { _backing: 5 };
a.value = 3;
Object.defineProperty(a, "value", { get() { return this._backing * 2; }, configurable: true });
console.log(a.value, a._backing);"#,
        ),
    ] {
        let vs = backing(src);
        assert!(
            !vs.is_empty() && vs.iter().all(|v| matches!(v, Verdict::No(_))),
            "{name}: {vs:?}"
        );
    }
}

/// Control: an accessor on a key no store writes keeps the other keys'
/// slots (the runtime keeps the object on its layout).
#[test]
fn an_accessor_on_a_new_key_keeps_the_layout() {
    let vs = backing(
        r#"const a: any = { _backing: 5 };
Object.defineProperty(a, "value", { get() { return this._backing * 2; }, configurable: true });
console.log(a.value, a._backing);"#,
    );
    assert!(vs.iter().any(|v| !matches!(v, Verdict::No(_))), "{vs:?}");
}
