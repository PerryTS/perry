//! A literal's own accessor keys are part of its per-unit contract: the
//! reader's stand-in holds them as own accessors, so a call through a
//! getter's result runs with `this` = the receiver (the reader escapes it)
//! instead of being dropped. With only the call rule (a definite own key
//! hides the chain for a call, `call_shadow`), the three fixtures below
//! prove `createIdentifier` per unit while node (26) rewrites it
//! (`console.log` prints 315000 4 for each); the accessor contract makes
//! them unproven again.

use super::call_shadow::{per_unit, whole};

/// `factory.createAllType()` calls what the getter returns with `this` =
/// the factory; the shape of TypeScript's `createNodeFactory`.
fn factory(globals: &str, getter_body: &str, extra: &str) -> String {
    format!(
        r#"
{globals}
function createFactory(flags: number) {{
  const factory = {{
    flags,
    createIdentifier(text: string) {{ return {{ kind: 80, text }}; }},
    {extra}
    get createAllType() {{ {getter_body} }},
  }};
  return factory;
}}
const factory = createFactory(4);
let n = 0;
for (let i = 0; i < 1000; i++) n += factory.createAllType().kind + factory.createIdentifier("a").text.length;
console.log(n, factory.flags);
"#
    )
}

/// An unknown function (stored on `globalThis`) rewrites `createIdentifier`
/// through `this`.
fn negmut0() -> String {
    factory(
        r#"(globalThis as any).f = function (this: any) { this.createIdentifier = () => ({ kind: 1, text: "xyz" }); return { kind: 312 }; };"#,
        "return (globalThis as any).f;",
        "",
    )
}

/// A known function of the owner rewrites it; no unknown code.
fn lacthis() -> String {
    factory(
        r#"function swap(this: any) { this.createIdentifier = (t: string) => ({ kind: 1, text: "xyz" }); return { kind: 312 }; }"#,
        "return swap;",
        "",
    )
}

fn negret() -> String {
    factory(
        "function swap(this: any) { this.createIdentifier = this.createOther; return { kind: 312 }; }",
        "return swap;",
        r#"createOther(text: string) { return { kind: 81, text: text + "zz" }; },"#,
    )
}

#[test]
fn a_getter_result_rewriting_the_method_through_an_unknown_function_stays_unproven() {
    assert!(!per_unit(&negmut0()));
}

#[test]
fn a_getter_result_rewriting_the_method_through_a_known_function_stays_unproven() {
    assert!(!per_unit(&lacthis()));
}

#[test]
fn a_getter_result_swapping_the_method_for_a_sibling_stays_unproven() {
    assert!(!per_unit(&negret()));
}

#[test]
fn a_getter_that_returns_a_fresh_arrow_costs_the_stub() {
    // The cost of the stub: the arrow does not touch `this`, but the stub's
    // result is unknown, so the call escapes the receiver (precise form: a
    // later PR).
    let src = factory("", "return () => ({ kind: 312 });", "");
    assert!(!per_unit(&src));
}

#[test]
fn whole_program_mode_is_unchanged_by_the_contract() {
    // Whole program: the program's own `swap` is seen as the getter's
    // result, so the rewrite is proven (a set), not unknown.
    assert!(whole(&lacthis()).0);
    assert!(!whole(&negmut0()).0);
}
