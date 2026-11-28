//! Rule RP2 with a mixin parent (PERFNOW27): a class whose `extends` value
//! is a program class returned by a function (`class C extends Base()`)
//! is no Array subclass while that value holds no Array parent, so a
//! definition under an unknown key on its prototype does not change which
//! `@@iterator` an Array finds. The `Object.entries` pair stays index-exact
//! for the guarded `[k, v]` record: `k` is the key only, never the value.

use super::*;

fn verdict(base: &str) -> Verdict {
    let src = format!(
        r#"
        class Svc {{ size = 3; }}
        function g(a: any) {{ return a.size; }}
        const s = new Svc();
        declare function opaque(): any; const unknown: any = opaque();
        const take = (x: any) => {{ unknown.take(x); }};
        function Base(): any {{ {base} }}
        class P extends Base() {{ q() {{ return 2; }} }}
        Object.defineProperty(P.prototype, process.argv[3], {{ value: 1 }});
        const o: Record<string, unknown> = {{ a: s }};
        for (const [k, v] of Object.entries(o)) take(k);
        console.log(g(s), new P().q());
        "#
    );
    site(&facts(&src), "size").verdict.clone()
}

/// node: `k` is "a"; `s` never reaches `take`.
#[test]
fn mixin_parent_keeps_entry_key_exact() {
    let v = verdict("return class B { m() { return 1; } };");
    assert!(!matches!(v, Verdict::No(_)), "{v:?}");
}

/// An `extends` value that may be `Array`: the definition lands on
/// `P.prototype`, which only `P` instances inherit. The `Object.entries`
/// pair is born an Array of %Array.prototype% (CreateArrayFromList), so its
/// `@@iterator` is the intrinsic one and the record reads exact positions
/// (DESTR-POS): `k` is the key, `s` never reaches `take`.
#[test]
fn array_parent_keeps_the_pair_exact() {
    let v = verdict("return Array;");
    assert!(!matches!(v, Verdict::No(_)), "{v:?}");
}
