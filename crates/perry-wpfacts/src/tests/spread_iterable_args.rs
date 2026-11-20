//! A syntactic spread argument passes what iterating its value yields
//! (CreateListFromIterable, ECMA-262 13.3.8.1): the call site's spread entry
//! is the list of those values, read at its elements like every other
//! spread entry (`apply`'s array-like, `Reflect.apply`'s argumentsList, a
//! bound call's list). Spreading an iterable passes its `next().value`s,
//! never the iterable's own fields.

use super::*;

/// Every site of `key` may see `want` (or ⊤): never a proof that leaves
/// out a value the call passes.
fn sees(src: &str, key: &str, want: &str) {
    let pf = facts(src);
    let s: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    assert!(!s.is_empty(), "site {key} in\n{}", pf.dump_text());
    for s in s {
        assert!(
            s.top || s.receivers.iter().any(|r| r.ends_with(want)),
            "{key}: {:?} top={} misses {want}\n{}",
            s.receivers,
            s.top,
            pf.dump_text()
        );
    }
}

const BAG: &str = r#"
    class Box { v = 1; w = 2; }
    class Cup { w = "x"; z = 3; }
    class Bag {
      items: Cup[] = [new Cup()];
      [Symbol.iterator]() {
        let i = 0;
        const items = this.items;
        return { next: () => i < items.length ? { value: items[i++], done: false } : { value: undefined, done: true } };
      }
    }
"#;

/// node: prints `2 x`.
#[test]
fn a_spread_user_iterable_passes_its_values() {
    let src = format!(
        "{BAG} function f(a: any) {{ return a.w; }} console.log(f(new Box()), f(...new Bag()));"
    );
    sees(&src, "w", ":Cup");
}

/// node: prints `2 x`. (The holder keeps the value under `h`, so the
/// only `w` site is the constructor's read of its parameter.)
#[test]
fn a_spread_user_iterable_passes_its_values_to_a_constructor() {
    let src = format!(
        "{BAG} class Holder {{ h: any; constructor(a: any) {{ this.h = a.w; }} }} console.log(new Holder(new Box()).h, new Holder(...new Bag()).h);"
    );
    sees(&src, "w", ":Cup");
}

/// node: prints `2 x`. (The test harness does not lower generators: the
/// yield escapes, so the site may be ⊤; it must not be a proof without Cup.)
#[test]
fn a_spread_generator_passes_its_yields() {
    sees(
        r#"
        class Box { v = 1; w = 2; }
        class Cup { w = "x"; z = 3; }
        function* g(): Generator<any> { yield new Cup(); }
        function f(a: any) { return a.w; }
        console.log(f(new Box()), f(...g()));
        "#,
        "w",
        ":Cup",
    );
}

/// node: prints `x`.
#[test]
fn a_spread_set_passes_its_values() {
    sees(
        r#"
        class Cup { w = "x"; }
        function f(a: any) { return a.w; }
        console.log(f(...new Set<any>([new Cup()])));
        "#,
        "w",
        ":Cup",
    );
}

/// Control (node: prints `2`): a spread array still passes its elements.
#[test]
fn a_spread_array_passes_its_elements() {
    sees(
        r#"
        class Box { v = 1; w = 2; }
        function f(a: any) { return a.w; }
        const xs = [new Box()];
        console.log(f(...xs));
        "#,
        "w",
        ":Box",
    );
}
