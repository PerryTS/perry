//! A for-of over a proven array is a private record: the payload holds the
//! array or its iterator, and the element is `payload[arrayRecordIndex(i)]`.
//! The glue calls are what codegen lowers them to, and a union of primitive
//! kinds is exactly that union (CW2 F, DECISIONS 60).

use super::*;

/// Bounce's loop: the cursor is a Number, so the element read is the
/// elements alone, not the iterator's `next` (a builtin function value).
#[test]
fn for_of_record_element_is_the_elements() {
    let pf = facts(
        r#"
        class Ball {
          x: number;
          constructor(x: number) { this.x = x; }
          bounce(): boolean { this.x += 1; return this.x > 3; }
        }
        function run(n: number) {
          const balls = new Array(n);
          for (let i = 0; i < n; i += 1) balls[i] = new Ball(i);
          let b = 0;
          for (const ball of balls) { if (ball.bounce()) b += 1; }
          return b;
        }
        console.log(run(process.argv.length + 4));
        "#,
    );
    let st = site(&pf, "bounce");
    assert!(!st.top, ".bounce receiver carries TOP\n{}", pf.dump_text());
    assert!(
        matches!(st.verdict, Verdict::T0Mono | Verdict::T0Set),
        ".bounce: {:?}\n{}",
        st.verdict,
        pf.dump_text()
    );
}

/// ToPropertyKey(true) is "true": a Boolean index reads the named member,
/// here a method on the prototype, so the field is not `undefined` alone.
#[test]
fn boolean_index_reads_the_named_member() {
    let pf = facts(
        r#"
        class K { true(): number { return 1; } }
        class H {
          m: any;
          constructor(k: K, b: boolean) { this.m = (k as any)[b as any]; }
        }
        const h = new H(new K(), process.argv.length > 1);
        console.log(typeof h.m);
        "#,
    );
    let t = tmpl(&pf, "H");
    let d = field(t, "m").kind.desc();
    assert_ne!(d, "undef", "k[bool] lost K.prototype.true\n{}", pf.dump_text());
}

/// A union of primitive kinds is that union, not every kind: an enum
/// member is a Number or a String, never a Boolean.
#[test]
fn union_of_kinds_is_exact() {
    let pf = facts(
        r#"
        enum E { A = 1, B = "b" }
        class H {
          e: any;
          constructor(k: E) { this.e = k === E.A ? E.A : E.B; }
        }
        const h = new H(process.argv.length > 1 ? E.A : E.B);
        console.log(h.e);
        "#,
    );
    let t = tmpl(&pf, "H");
    let d = field(t, "e").kind.desc();
    assert!(!d.contains("bool"), "E member kind {d}\n{}", pf.dump_text());
}

/// Richards' `data[count]` with `count` possibly absent: the key
/// "undefined" is no builtin prototype member, so the element read stays
/// the array's own elements and `undefined`.
#[test]
fn undefined_index_on_array_reads_no_builtin_member() {
    let pf = facts(
        r#"
        class P { v: number; constructor(v: number) { this.v = v; } get(): number { return this.v; } }
        class R { count?: number; constructor(c: boolean) { if (c) this.count = 0; } }
        const data = [new P(1), new P(2)];
        const r = new R(process.argv.length > 1);
        const x = data[r.count as number];
        console.log(x ? x.get() : 0);
        "#,
    );
    let st = site(&pf, "get");
    assert!(!st.top, ".get receiver carries TOP\n{}", pf.dump_text());
    assert!(
        matches!(st.verdict, Verdict::T0Mono | Verdict::T0Set),
        ".get: {:?}\n{}",
        st.verdict,
        pf.dump_text()
    );
}
