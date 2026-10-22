//! `Array(x)` / `new Array(x)` with one argument: a Number is the length
//! (holes, no element), any other value is the one element (ECMA-262
//! Array). The length is not an element (DECISIONS 51).

use super::*;

/// A hash table's buckets: `new Array(n)` holds no number, so a bucket read
/// holds only entries and holes. With the length as an element, `cur.next`
/// on a number read a builtin function, `cur.match(h, k)` on it passed `k`
/// to unknown code, and every key class escaped.
#[test]
fn array_length_is_not_an_element() {
    let pf = facts(
        r#"
        class K { v: number; constructor(v: number) { this.v = v; } hash() { return this.v; } }
        class E {
          h: number; k: K; next: E | null;
          constructor(h: number, k: K, n: E | null) { this.h = h; this.k = k; this.next = n; }
          match(h: number, k: K) { return this.h === h && this.k === k; }
        }
        class D {
          b: any[];
          constructor() { this.b = new Array(4); }
          insert(k: K) {
            const h = k.hash();
            const i = h & (this.b.length - 1);
            let cur = this.b[i];
            if (!cur) { this.b[i] = new E(h, k, null); return; }
            while (true) {
              if (cur.match(h, k)) { return; }
              if (!cur.next) { cur.next = new E(h, k, null); return; }
              cur = cur.next;
            }
          }
        }
        const d = new D();
        const k = new K(1);
        d.insert(k); d.insert(new K(5)); d.insert(k);
        console.log(k.v);
    "#,
    );
    assert_eq!(tmpl(&pf, "K").escape, None, "{}", pf.dump_text());
    assert_eq!(tmpl(&pf, "E").escape, None, "{}", pf.dump_text());
    let m = site(&pf, "match");
    assert_eq!(m.receivers, vec!["t.ts:E".to_string()], "{}", pf.dump_text());
}

/// One argument that is not a Number is the array's only element.
#[test]
fn array_of_one_object_holds_it() {
    let pf = facts(
        r#"
        class A { x: number; constructor() { this.x = 1; } }
        const a: any[] = new Array(new A());
        console.log(a[0].x);
    "#,
    );
    assert_eq!(site(&pf, "x").verdict, Verdict::T0Mono, "{}", pf.dump_text());
}
