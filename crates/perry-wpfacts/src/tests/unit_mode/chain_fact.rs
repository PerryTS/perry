//! DECISIONS 242: a prototype's chain fact reaches an object only through a
//! key the object does not own. A defineProperty under a computed key that
//! meets the `Object.prototype` token (on it directly, or through a
//! parameter another unit's caller gives it) is an accessor under any key
//! there, in both modes; it reaches a template only through a miss site (a
//! [[Get]], call or [[Set]] of a key it does not own definitely) or keys
//! made by [[Set]] (a constructor's `this.k =`), and then escapes it in both
//! modes.

use super::*;

/// Unit a: a computed-key accessor define on `Object.prototype` itself.
const DIRECT_DEF: &str = r#"export function def(k: string) {
  Object.defineProperty(Object.prototype, k, { get() { return 7; }, configurable: true });
}"#;

/// Unit a: a computed-key accessor define on whatever its caller passes;
/// main passes the `Object.prototype` token itself.
const PARAM_DEF: &str = r#"export function def(x: any, k: string) {
  Object.defineProperty(x, k, { get() { return 7; }, configurable: true });
}"#;

const MAIN_DIRECT: &str = r#"import { def } from "./a";
import { mk } from "./b";
def(String(mk(1, "c")));
console.log(mk(2, "d"));"#;

const MAIN_PARAM: &str = r#"import { def } from "./a";
import { mk } from "./b";
def(Object.getPrototypeOf({}), String(mk(1, "c")));
console.log(mk(2, "d"));"#;

/// The two producers: on the token, through a parameter (G1).
const HOW: [(&str, &str, &str); 2] = [
    ("direct", DIRECT_DEF, MAIN_DIRECT),
    ("param", PARAM_DEF, MAIN_PARAM),
];

/// The literal `{ a, b }` templates, by (prefix_stable, fixed_layout, escape).
fn lit(pf: &ProgramFacts) -> Vec<(bool, bool, Option<String>)> {
    let t: Vec<_> = pf
        .templates
        .iter()
        .filter(|t| t.birth_keys == ["a", "b"])
        .map(|t| (t.prefix_stable, t.fixed_layout, t.escape.clone()))
        .collect();
    assert!(!t.is_empty(), "the literal\n{}", pf.dump_text());
    t
}

fn birth_safe(pf: &ProgramFacts) -> bool {
    lit(pf).iter().all(|(p, f, e)| *p && *f && e.is_none())
}

/// Escaped through the chain: by the accessor itself, or (a call of a key
/// the literal does not own) as the receiver of the unknown function the
/// accessor returns.
fn chain_escaped(pf: &ProgramFacts) -> bool {
    lit(pf).iter().all(|(p, _, e)| {
        !*p && matches!(
            e.as_deref(),
            Some("accessor under a computed key on the chain")
                | Some("receiver of a call of an unknown function value")
        )
    })
}

fn modes(mods: &[(&str, &str)]) -> (ProgramFacts, ProgramFacts) {
    (whole_of(mods), units_of(mods))
}

const BIRTH: &str =
    r#"export function mk(n: number, _k: string) { const o = { a: n, b: 2 }; return o.a + o.b; }"#;
const COMPUTED_MISS: &str = r#"export function mk(n: number, k: string) {
  const o = { a: n, b: 2 };
  return o.a + o.b + ((o as any)[k] === undefined ? 0 : 1);
}"#;
const NAMED_MISS: &str = r#"export function mk(n: number, _k: string) {
  const o = { a: n, b: 2 };
  return o.a + o.b + String(o.hasOwnProperty("a")).length;
}"#;
/// `constructor` is the one key a program may define on `Object.prototype`
/// (DEC 250: the lock exempts it, every other add or member write there is
/// a TypeError): a named miss under it still meets the accessor.
const CTOR_MISS: &str = r#"export function mk(n: number, _k: string) {
  const o = { a: n, b: 2 };
  return o.a + o.b + String((o as any).constructor).length;
}"#;
/// A [[Set]] under a key no constant names: a constant other than
/// `constructor` is final on `Object.prototype` (DEC 250).
const SET_MISS: &str = r#"export function mk(n: number, k: string) {
  const o = { a: n, b: 2 };
  (o as any)[k + process.argv[2]] = 1;
  return o.a + o.b;
}"#;

/// The literal's birth keys are defined, no prototype accessor runs for
/// them: proven in both modes (main escaped the literal in whole).
#[test]
fn birth_keys_of_a_literal_do_not_depend_on_an_any_key_accessor() {
    for (how, a, main) in HOW {
        let (w, u) = modes(&[("a", a), ("b", BIRTH), ("main", main)]);
        assert!(
            birth_safe(&w),
            "{how} whole {:?}\n{}",
            lit(&w),
            w.dump_text()
        );
        assert!(
            birth_safe(&u),
            "{how} per unit {:?}\n{}",
            lit(&u),
            u.dump_text()
        );
        assert!(
            proven(&w, "a") && proven(&w, "b"),
            "{how} whole sites {:?}",
            verdicts(&w, "a")
        );
        assert!(
            proven(&u, "a") && proven(&u, "b"),
            "{how} unit sites {:?}",
            verdicts(&u, "a")
        );
    }
}

/// A computed or constructor miss may run the accessor with the literal
/// as this. DEC 250 keeps the named image member hasOwnProperty final, so
/// that call alone keeps the birth facts in both modes.
#[test]
fn a_miss_site_escapes_the_literal_in_both_modes() {
    for (name, b) in [
        ("computed", COMPUTED_MISS),
        ("named", NAMED_MISS),
        ("constructor", CTOR_MISS),
        ("set", SET_MISS),
    ] {
        for (how, a, main) in HOW {
            let (w, u) = modes(&[("a", a), ("b", b), ("main", main)]);
            if name == "named" {
                assert!(birth_safe(&w), "{how} named whole {:?}", lit(&w));
                assert!(birth_safe(&u), "{how} named per unit {:?}", lit(&u));
                assert_eq!(lit(&w), lit(&u), "{name}/{how}: whole = per unit");
                continue;
            }
            assert!(
                chain_escaped(&w),
                "{name}/{how} whole {:?}\n{}",
                lit(&w),
                w.dump_text()
            );
            assert!(
                chain_escaped(&u),
                "{name}/{how} per unit {:?}\n{}",
                lit(&u),
                u.dump_text()
            );
            assert_eq!(lit(&w), lit(&u), "{name}/{how}: whole = per unit");
        }
    }
}

/// DEC 250 (Q2): the runtime refuses a program accessor on
/// `Object.prototype` under any key but `constructor`, so a named read miss
/// under another key (`zz`) runs none: the literal keeps its birth facts in
/// both modes. Red before the reader: the miss escaped it.
#[test]
fn a_named_miss_meets_no_any_key_accessor_under_the_lock() {
    let b = r#"export function mk(n: number, _k: string) {
  const o = { a: n, b: 2 };
  return o.a + o.b + ((o as any).zz === undefined ? 0 : 1);
}"#;
    for (how, a, main) in HOW {
        let (w, u) = modes(&[("a", a), ("b", b), ("main", main)]);
        assert!(
            birth_safe(&w),
            "{how} whole {:?}\n{}",
            lit(&w),
            w.dump_text()
        );
        assert!(
            birth_safe(&u),
            "{how} per unit {:?}\n{}",
            lit(&u),
            u.dump_text()
        );
        assert_eq!(lit(&w), lit(&u), "{how}: whole = per unit");
    }
}

/// G2 (contract half): the miss site is in a third unit, a computed-key read
/// of a parameter that holds the literal. The callee's contract names it
/// (`reach_keys` `*`), the caller's unit runs it on the literal it passes.
/// The same program with an own-key read keeps the literal's birth facts.
#[test]
fn a_computed_read_in_another_units_callee_is_a_miss_on_the_argument() {
    let b = r#"import { look } from "./c";
export function mk(n: number, k: string) { const o = { a: n, b: 2 }; return o.a + o.b + look(o, k); }"#;
    let c =
        r#"export function look(o: any, k: string): number { return o[k] === undefined ? 0 : 1; }"#;
    let c0 =
        r#"export function look(o: any, _k: string): number { return o.a === undefined ? 0 : 1; }"#;
    for (how, a, main) in HOW {
        let (w, u) = modes(&[("a", a), ("c", c), ("b", b), ("main", main)]);
        assert!(
            chain_escaped(&w),
            "{how} whole {:?}\n{}",
            lit(&w),
            w.dump_text()
        );
        assert!(
            chain_escaped(&u),
            "{how} per unit {:?}\n{}",
            lit(&u),
            u.dump_text()
        );
        assert_eq!(lit(&w), lit(&u), "{how}: whole = per unit");
        let (w, u) = modes(&[("a", a), ("c", c0), ("b", b), ("main", main)]);
        assert!(
            birth_safe(&w) && birth_safe(&u),
            "{how} own key read: {:?} {:?}",
            lit(&w),
            lit(&u)
        );
    }
}

/// A constructor's `this.k = v` is a [[Set]]: a setter on the chain may take
/// it, so the instance depends on the chain (unproven) in both modes. Under
/// DEC 250 (Q2) a program setter on `Object.prototype` stands only under
/// `constructor`: `C` (keys `a`, `b`) keeps its facts, `D` (which also sets
/// `constructor`) still depends on the chain. Red before the reader: C was
/// unproven.
#[test]
fn constructor_set_keys_depend_on_the_chain() {
    let b = r#"class C { a: number; b: number; constructor(n: number) { this.a = n; this.b = 2; } }
class D { a: number; constructor(n: number) { this.a = n; (this as any).constructor = 1; } }
export function mk(n: number, _k: string) { const o = new C(n); const d = new D(n); return o.a + o.b + d.a; }"#;
    for (how, a, main) in HOW {
        let (w, u) = modes(&[("a", a), ("b", b), ("main", main)]);
        for pf in [&w, &u] {
            let of = |name: &str| -> Vec<_> {
                pf.templates
                    .iter()
                    .filter(|t| t.display.ends_with(name))
                    .map(|t| (t.prefix_stable, t.chain.clone(), t.escape.clone()))
                    .collect()
            };
            let (c, d) = (of(":C"), of(":D"));
            assert!(
                !c.is_empty() && !d.is_empty(),
                "{how} classes\n{}",
                pf.dump_text()
            );
            assert!(c.iter().all(|t| t.0), "{how} C {c:?}");
            assert!(d.iter().all(|t| !t.0), "{how} D {d:?}");
        }
    }
}

/// G1 must not over-fire: one helper `def(o, k, d)` in unit a, called from
/// unit b once with a literal and once with `Object.prototype`. Only the
/// `Object.prototype` call is the accessor under any key; the literal the
/// define lands on is what the whole program makes of it, and an unrelated
/// literal with no miss site keeps its birth facts, in both modes.
#[test]
fn a_parameter_define_fires_the_any_key_accessor_only_for_object_prototype() {
    let a = r#"export function def(o: any, k: string, d: any): void { Object.defineProperty(o, k, d); }"#;
    let b = r#"import { def } from "./a";
function rd(o: { a: number; b: number }): number { return o.a + o.b; }
export function mk(n: number, k: string) {
  const lit: any = { x: n, y: 2 };
  def(lit, "z", { value: 3, enumerable: true, configurable: true });
  const q = { a: n, b: 2 };
  if (k.length > 9) def(Object.prototype, k, { get() { return 7; }, configurable: true });
  return rd(q) + lit.x + lit.z;
}"#;
    let main = r#"import { mk } from "./b";
console.log(mk(1, String(process.argv.length)));"#;
    let (w, u) = modes(&[("a", a), ("b", b), ("main", main)]);
    assert!(birth_safe(&w), "whole {:?}\n{}", lit(&w), w.dump_text());
    assert!(birth_safe(&u), "per unit {:?}\n{}", lit(&u), u.dump_text());
    let xy = |pf: &ProgramFacts| -> Vec<_> {
        pf.templates
            .iter()
            .filter(|t| t.birth_keys == ["x", "y"])
            .map(|t| (t.prefix_stable, t.fixed_layout, t.escape.is_some()))
            .collect()
    };
    // The escape reasons differ by mode (whole: the descriptor's accessor;
    // per unit: the argument of another unit's function), the facts do not.
    assert_eq!(xy(&w), xy(&u), "the defined literal: whole = per unit");
}
