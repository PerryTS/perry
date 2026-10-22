//! An Array's own `length` is a Number and never an element (ECMA-262
//! Array exotic objects: `length` is a data property holding a uint32; a
//! store converts its value with ToNumber and only moves the end of the
//! element range). DECISIONS 54.

use super::*;

/// som.js `Vector`: growing copies the storage and sets its `length`. With
/// the length store an element, every element read held a Number, `a.w` on
/// it read a builtin function, `.forEach(cb)` on that passed `cb` to unknown
/// code, and its parameter `b` was ⊤ (deltablue.js `v.constraints.forEach`).
#[test]
fn length_store_is_not_an_element() {
    let pf = facts(
        r#"
        class B { m() { return 1; } }
        class V {
          s: any[]; n: number;
          constructor() { this.s = new Array(2); this.n = 0; }
          add(e: any) {
            if (this.n >= this.s.length) {
              const L = this.s.length * 2;
              this.s = this.s.slice();
              this.s.length = L;
            }
            this.s[this.n] = e;
            this.n += 1;
          }
          each(f: (x: any) => void) { for (let i = 0; i < this.n; i++) { f(this.s[i]); } }
        }
        class A { w: B[]; constructor() { this.w = [new B()]; } }
        const v = new V();
        for (let i = 0; i < 5; i++) { v.add(new A()); }
        let t = 0;
        v.each((a: any) => { a.w.forEach((b: any) => { t += b.m(); }); });
        console.log(t);
    "#,
    );
    assert_eq!(tmpl(&pf, "B").escape, None, "{}", pf.dump_text());
    assert_eq!(site(&pf, "m").verdict, Verdict::T0Mono, "{}", pf.dump_text());
}

/// A read of `length` is that Number only: it does not read the elements.
/// With the elements in it, `n.x = 5` was a post-construction store into
/// `A.x`.
#[test]
fn length_read_is_not_the_elements() {
    let pf = facts(
        r#"
        class A { x: number; constructor() { this.x = 1; } }
        function put(a: any[], i: number) { a[i] = new A(); }
        const a: any[] = [];
        put(a, 0);
        const n: any = a.length;
        n.x = 5;
        console.log(a[0].x);
    "#,
    );
    let x = field(tmpl(&pf, "A"), "x");
    assert!(x.readonly && x.writers_post == 0, "{}", pf.dump_text());
}
