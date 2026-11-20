//! The array search methods the HIR folds (`Expr::ArrayIndexOf`,
//! `ArrayLastIndexOf`, `ArrayIncludes`) read the receiver's elements and
//! compare them with the value: no call, no conversion but `fromIndex`'s
//! (ECMA-262 23.1.3.13/.16/.19). Neither the array nor the value escapes.

use super::*;

/// `a.indexOf(q)` / `a.includes(q)` / `a.lastIndexOf(q, 0)`: `P` stays
/// unescaped, so `a[0].v` and `q.v` are proven. As a mixed primitive operand
/// the receiver and the value reached a builtin protocol call and escaped,
/// and the index store `a[i] = ...` on the escaped array was a ⊤ write.
#[test]
fn folded_search_does_not_escape_the_array_or_the_value() {
    let pf = facts(
        r#"
        class P { v: number; constructor() { this.v = 1; } }
        const a: P[] = [new P()];
        const q = new P();
        function f(i: number): number { a[i] = new P(); return a.indexOf(q); }
        const n = f(1) + a.lastIndexOf(q, 0) + (a.includes(q) ? 1 : 0);
        console.log(n, a[0].v, q.v);
    "#,
    );
    assert_eq!(tmpl(&pf, "P").escape, None, "{}", pf.dump_text());
    let v: Vec<_> = pf.sites.iter().filter(|s| s.key == "v").collect();
    assert!(
        v.len() >= 2 && v.iter().all(|s| s.verdict == Verdict::T0Mono),
        "{}",
        pf.dump_text()
    );
}

/// `fromIndex` is converted: an object's `valueOf` runs.
#[test]
fn folded_search_converts_from_index() {
    let pf = facts(
        r#"
        class I { k: number; constructor() { this.k = 0; } valueOf() { this.k = 2; return 0; } }
        const a: number[] = [1, 2];
        const i = new I();
        console.log(a.indexOf(2, i as any), i.k);
    "#,
    );
    let k = field(tmpl(&pf, "I"), "k");
    assert!(k.writers_post > 0, "{}", pf.dump_text());
}
