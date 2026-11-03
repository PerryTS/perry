//! The global conversions and predicates fix the kind of their result
//! (ECMA-262): `parseInt` is a Number, `String(x)` a String, `isNaN` a
//! Boolean, `BigInt(x)` a BigInt (DECISIONS 52).

use super::*;

/// Each operation's result is one primitive kind.
#[test]
fn conversion_results_have_one_kind() {
    let pf = facts(
        r#"
        class A {
          a: number; b: string; c: boolean; d: bigint; e: number;
          constructor(s: string) {
            this.a = parseInt(s, 10);
            this.b = String(s);
            this.c = isNaN(this.a);
            this.d = BigInt(this.a);
            this.e = parseFloat(s);
          }
        }
        const x = new A(process.argv.length > 9 ? "7" : "5");
        console.log(x.a, x.b, x.c, x.d, x.e);
    "#,
    );
    let t = tmpl(&pf, "A");
    for (k, want) in [
        ("a", "num"),
        ("b", "str"),
        ("c", "bool"),
        ("d", "bigint"),
        ("e", "num"),
    ] {
        assert_eq!(field(t, k).kind.desc(), want, "{k}\n{}", pf.dump_text());
    }
}

/// DeltaBlue's chain: `new Array(n + 1)` with `n` from `parseInt` holds no
/// string, so its element read is the class instance alone.
#[test]
fn array_sized_by_a_parsed_count_holds_only_elements() {
    let pf = facts(
        r#"
        class V { value: number; constructor() { this.value = 0; } }
        function run(n: number) {
          const vars = new Array(n + 1);
          for (let i = 0; i < n + 1; i += 1) { vars[i] = new V(); }
          return vars[n].value;
        }
        console.log(run(parseInt(process.argv[2] || "3", 10)));
    "#,
    );
    let s = site(&pf, "value");
    assert_eq!(
        s.receivers,
        vec!["t.ts:V".to_string()],
        "{}",
        pf.dump_text()
    );
    assert!(!s.top, "{}", pf.dump_text());
}

/// Every Math function returns a Number, through the HIR operation and
/// through the `Math` namespace value (DECISIONS 53).
#[test]
fn math_results_are_numbers() {
    let pf = facts(
        r#"
        const M: any = Math;
        class A {
          a: number; b: number;
          constructor(s: string) { this.a = Math.max(s.length, 4); this.b = M.max(s.length, 4); }
        }
        const x = new A(process.argv.length > 9 ? "7" : "5");
        console.log(x.a, x.b);
    "#,
    );
    let t = tmpl(&pf, "A");
    for k in ["a", "b"] {
        assert_eq!(field(t, k).kind.desc(), "num", "{k}\n{}", pf.dump_text());
    }
}

/// som.js Vector: `new Array(Math.max(idx + 1, 4))` holds only what is
/// stored, never a string from the length.
#[test]
fn array_sized_by_math_max_holds_only_elements() {
    let pf = facts(
        r#"
        class V { value: number; constructor() { this.value = 0; } }
        function grow(idx: number) {
          const s = new Array(Math.max(idx + 1, 4));
          s[idx] = new V();
          return s[idx].value;
        }
        console.log(grow(parseInt(process.argv[2] || "3", 10)));
    "#,
    );
    let s = site(&pf, "value");
    assert_eq!(
        s.receivers,
        vec!["t.ts:V".to_string()],
        "{}",
        pf.dump_text()
    );
}
