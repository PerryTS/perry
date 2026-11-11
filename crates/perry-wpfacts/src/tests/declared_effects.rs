//! A built-in that declares its effects where the runtime defines it runs
//! through one interpreter of the declaration (`builtins::effects`); the
//! Array methods whose hand models it replaced keep their facts. Receivers
//! are `any` so the calls stay calls of the native (not HIR array folds).

use super::*;
use crate::builtins::effects_for_tests::{declared_array_method, interpretable};
use crate::model::prim;
use perry_hir::realm_manifest::{RealmEffect, RealmOperand, RealmSlot, RealmSlotRoot};
use std::borrow::Cow;

fn sites<'a>(pf: &'a ProgramFacts, key: &str) -> Vec<&'a crate::SiteFacts> {
    let v: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    assert!(!v.is_empty(), "site {key} in\n{}", pf.dump_text());
    v
}

/// Every site of `key` holds exactly the templates `want`, no ⊤.
fn holds(pf: &ProgramFacts, key: &str, want: &[&str]) {
    for s in sites(pf, key) {
        let mut w: Vec<String> = want.iter().map(|x| format!("t.ts:{x}")).collect();
        w.sort();
        let mut got = s.receivers.clone();
        got.sort();
        assert!(
            !s.top && got == w,
            "{key}: {:?} top={} want {w:?}\n{}",
            s.receivers,
            s.top,
            pf.dump_text()
        );
    }
}

const P: &str = r#"
    class P { v = 1; }
    class Q { w = 2; }
    class T { t = 3; }
    function arr(): any { return [new P()]; }
"#;

/// The migrated methods are declared in forms the interpreter runs.
/// forEach keeps its hand arm; map and filter construct their species
/// through the interpreter.
#[test]
fn the_array_declarations_are_interpretable() {
    for n in [
        "push", "reverse", "every", "some", "find", "indexOf", "includes", "join", "sort",
        "forEach", "map", "filter",
    ] {
        assert!(declared_array_method(n), "Array.prototype.{n}");
    }
    for n in ["slice", "concat", "pop", "splice"] {
        assert!(
            !declared_array_method(n),
            "Array.prototype.{n} is undeclared"
        );
    }
}

/// `open` and an empty declaration keep today's arm: the interpreter
/// refuses them. A species construction runs as the result, of the
/// receiver only.
#[test]
fn open_keeps_the_hand_arm_species_of_the_receiver_runs() {
    let this = RealmSlot {
        root: RealmSlotRoot::This,
        steps: Cow::Borrowed(&[]),
    };
    assert!(!interpretable(&[RealmEffect::Open]));
    assert!(!interpretable(&[]));
    let species = |of: RealmSlot| {
        RealmEffect::Returns(RealmOperand::Species {
            of,
            args: Cow::Borrowed(&[]),
            elems: Cow::Borrowed(&[]),
        })
    };
    assert!(interpretable(&[species(this.clone())]));
    let arg = RealmSlot {
        root: RealmSlotRoot::Arg(0),
        steps: Cow::Borrowed(&[]),
    };
    assert!(!interpretable(&[species(arg)]));
    assert!(interpretable(&[RealmEffect::Returns(RealmOperand::Slot(
        this
    ))]));
}

/// `calls(arg(0), this = arg(1), args = [elems(this), ..])` (find): the
/// callback receives the elements and the declared `this`.
#[test]
fn calls_passes_this_and_arguments() {
    let pf = facts(&format!(
        "{P}
        const a = arr();
        a.find(function (this: any, x: any) {{ console.log(x.v, this.t); return true; }}, new T());"
    ));
    holds(&pf, "v", &["P"]);
    holds(&pf, "t", &["T"]);
}

/// `map` runs its declaration (its result is the species construction's):
/// the callback receives the elements and `this`, the result holds what
/// the callback returned.
#[test]
fn map_runs_its_declaration() {
    let pf = facts(&format!(
        "{P}
        const a = arr();
        const r = a.map(function (this: any, x: any) {{ console.log(x.v, this.t); return new Q(); }}, new T());
        console.log(r[0].w);"
    ));
    holds(&pf, "v", &["P"]);
    holds(&pf, "t", &["T"]);
    holds(&pf, "w", &["Q"]);
}

/// `stores(rest(0) -> elems(this))` (push): what is pushed is read back
/// from the receiver; a spread argument's values reach it too (a load of
/// any key of the spread array, as the hand model had it).
#[test]
fn stores_rest_into_the_receivers_elements() {
    let pf = facts(&format!(
        "{P}
        const a: any = [];
        a.push(new P(), new T());
        console.log(a[0].v);"
    ));
    holds(&pf, "v", &["P", "T"]);
    assert!(tmpl(&pf, "P").escape.is_none(), "{}", pf.dump_text());
    let pf = facts(&format!(
        "{P}
        const a: any = [];
        const b: any = [new Q()];
        a.push(...b);
        console.log(a[0].w);"
    ));
    let w = sites(&pf, "w");
    assert!(
        w.iter()
            .all(|s| s.receivers.contains(&"t.ts:Q".to_string())),
        "{}",
        pf.dump_text()
    );
}

/// `filter` runs its declaration (its result is the species
/// construction's): a new array holding the receiver's elements; a store
/// into it does not reach the receiver.
#[test]
fn filter_runs_its_declaration() {
    let pf = facts(&format!(
        "{P}
        const a = arr();
        const r = a.filter((x: any) => x.v > 0);
        r[1] = new Q();
        console.log(r[0].v, a[0].v);"
    ));
    let v = sites(&pf, "v");
    let last = v.iter().max_by_key(|s| s.byte_offset).unwrap();
    assert_eq!(
        last.receivers,
        vec!["t.ts:P".to_string()],
        "{}",
        pf.dump_text()
    );
    assert!(!last.top, "{}", pf.dump_text());
}

/// `returns(elems(this))`, `returns(this)`: find and reverse.
#[test]
fn returns_a_slot() {
    let pf = facts(&format!(
        "{P}
        const a = arr();
        const f = a.find((x: any) => x.v > 0);
        console.log(f.v, a.reverse()[0].v, a.some((x: any) => x.v > 0), a.every((x: any) => x.v > 0));"
    ));
    holds(&pf, "v", &["P"]);
}

/// `coerce(arg(1), ToPrimitive)` (indexOf, includes): the fromIndex's
/// `valueOf` runs; the searched value is not converted.
#[test]
fn coerce_runs_the_named_protocol_on_the_slot() {
    let pf = facts(&format!(
        "{P}
        class I {{ k: number; constructor() {{ this.k = 0; }} valueOf() {{ this.k = 2; return 0; }} }}
        class S {{ s: number; constructor() {{ this.s = 0; }} valueOf() {{ this.s = 2; return 0; }} }}
        const a = arr();
        console.log(a.indexOf(new S(), new I()), a.includes(new S(), new I()));"
    ));
    assert!(
        field(tmpl(&pf, "I"), "k").writers_post > 0,
        "{}",
        pf.dump_text()
    );
    assert_eq!(
        field(tmpl(&pf, "S"), "s").writers_post,
        0,
        "{}",
        pf.dump_text()
    );
}

/// `returns(prim(num))` (indexOf), `returns(prim(bool))` (includes): a field
/// holding only the search's result holds one primitive kind (the hand
/// model gave both kinds to each). The twin field holds either result and
/// keeps both kinds.
#[test]
fn a_search_result_is_one_primitive_kind() {
    let pf = facts(&format!(
        "{P}
        declare const k: number;
        class C {{ n: any; b: any; e: any; constructor() {{ this.n = 0; this.b = false; this.e = 0; }} }}
        const a = arr();
        const q = new Q();
        const c = new C();
        c.n = a.indexOf(q);
        c.b = a.includes(q);
        c.e = k > 1 ? a.indexOf(q) : a.includes(q);
        console.log(c.n, c.b, c.e);"
    ));
    let t = tmpl(&pf, "C");
    assert_eq!(field(t, "n").kind.desc(), "num", "{}", pf.dump_text());
    assert_eq!(field(t, "b").kind.desc(), "bool", "{}", pf.dump_text());
    assert_eq!(
        field(t, "e").kind.prims,
        prim::NUM | prim::BOOL,
        "{}",
        pf.dump_text()
    );
}

/// `coerce(elems(this))` and `coerce(arg(0))` (join): the elements' and
/// the separator's conversions run; a second argument is not converted.
#[test]
fn join_converts_the_elements_and_the_separator_only() {
    let pf = facts(
        r#"
        class E { e: number; constructor() { this.e = 0; } toString() { this.e = 1; return "e"; } }
        class D { d: number; constructor() { this.d = 0; } toString() { this.d = 1; return ","; } }
        class X { x: number; constructor() { this.x = 0; } toString() { this.x = 1; return ""; } }
        function arr(): any { return [new E()]; }
        console.log(arr().join(new D(), new X()));
    "#,
    );
    assert!(
        field(tmpl(&pf, "E"), "e").writers_post > 0,
        "{}",
        pf.dump_text()
    );
    assert!(
        field(tmpl(&pf, "D"), "d").writers_post > 0,
        "{}",
        pf.dump_text()
    );
    assert_eq!(
        field(tmpl(&pf, "X"), "x").writers_post,
        0,
        "{}",
        pf.dump_text()
    );
}

/// forEach keeps its hand arm: an array's callback gets a number as its
/// second argument (the declaration's `prim(num) | elems(this)` would pass
/// the elements there too).
#[test]
fn for_each_keeps_its_hand_arm() {
    let pf = facts(&format!(
        "{P}
        const a = arr();
        a.forEach((x: any, i: any) => console.log(x.v, i.v));"
    ));
    let v = sites(&pf, "v");
    assert!(
        v.iter().any(|s| s.receivers.is_empty()),
        "the index holds no element: {}",
        pf.dump_text()
    );
}

/// The declared coerce(returned(0), ToPrimitive) converts comparator results.
#[test]
fn sort_coerces_its_declared_callback_result() {
    let pf = facts(
        r#"
        class I { k: number; constructor() { this.k = 0; } valueOf() { this.k = 2; return 0; } }
        function arr(): any { return [1, 2]; }
        const i = new I();
        arr().sort(() => i);
        console.log(i.k);
    "#,
    );
    assert!(
        field(tmpl(&pf, "I"), "k").writers_post > 0,
        "{}",
        pf.dump_text()
    );
}

/// The no-comparator call runs the declared coercion of elements as well.
#[test]
fn sort_default_coerces_elements() {
    let pf = facts(
        r#"
        class I { k: number; constructor() { this.k = 0; } toString() { this.k = 2; return "x"; } }
        const i = new I();
        function arr(): any { return [i, i]; }
        arr().sort();
        console.log(i.k);
    "#,
    );
    assert!(
        field(tmpl(&pf, "I"), "k").writers_post > 0,
        "{}",
        pf.dump_text()
    );
}
