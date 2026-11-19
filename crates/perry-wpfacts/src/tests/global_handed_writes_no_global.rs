//! R-GLOBAL (owner decision 2026-11-18, closed world 2026-11-14): the global object
//! given to unknown code is no write of it under any key. It puts ⊤ in
//! reach of the program's stores into ⊤ (`Model::global_in_top`), each
//! recorded under its own key; the program's own stores of a global keep
//! their values. node: `unknown.take2(globalThis)` with no such store
//! leaves `Object` the intrinsic and `g` the program's `A`.
//!
//! Step 2 (owner decision 2026-11-18, closed world 11-14): unknown code
//! that holds the realm (any unknown call) replaces no key of the global
//! object either (`realm_replaced`), `globalThis` included.

use super::*;

const PRE: &str = r#"
    class A { zz = 1; }
    class B { zz = "b"; yy = 2; }
    const unknown: any = (globalThis as any)[process.argv[2]];
    (globalThis as any).g = new A();
"#;

const READ_G: &str = "console.log((globalThis as any).g.zz);";
// `Object.keys(x)` is the compiled operation (no member read); read it off a
// local so that `O.keys` is a site whose receiver is the global binding.
const READ_OBJECT: &str = "const O: any = Object; console.log(O.keys(new A()).length);";


fn sites<'a>(pf: &'a ProgramFacts, key: &str) -> Vec<&'a SiteFacts> {
    let s: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    assert!(!s.is_empty(), "no site {key}\n{}", pf.dump_text());
    s
}

fn proven(pf: &ProgramFacts, key: &str) -> bool {
    sites(pf, key)
        .iter()
        .all(|s| !s.top && matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
}

/// The `g.zz` read may see a `B` (or ⊤): the program's value is there.
fn g_sees_b(pf: &ProgramFacts) -> bool {
    sites(pf, "zz")
        .iter()
        .any(|s| s.top || s.receivers.iter().any(|r| r.ends_with(":B") || r.contains(":B#")))
}

fn both_orders(hand: &str, body: &str) -> [String; 2] {
    [format!("{PRE}{hand}\n{body}"), format!("{PRE}{body}\n{hand}")]
}

/// No site through the read is ⊤: `O` is the intrinsic `Object`, `g` the
/// program's `A`.
fn known(pf: &ProgramFacts) -> bool {
    sites(pf, "keys").iter().all(|s| !s.top) && sites(pf, "zz").iter().all(|s| !s.top) && !g_sees_b(pf)
}

/// RED: the global object handed, no store into ⊤: `Object` stays the
/// intrinsic, `g` the program's `A`. (Was: ⊤, the escape recorded an
/// any-key unmodeled write of the global object, and the call of unknown
/// code holding the realm replaced every key of it, `realm_replaced`.)
#[test]
fn a_global_read_stays_known_after_the_global_object_is_handed() {
    for src in both_orders("unknown.take2(globalThis);", &format!("{READ_OBJECT}\n{READ_G}")) {
        let pf = facts(&src);
        assert!(known(&pf), "{src}\n{}", pf.dump_text());
    }
}

/// RED (step 2): a call of an unknown native holds the realm; it replaces
/// no key of the global object (owner decision 2026-11-18): `globalThis.Object`
/// stays the intrinsic and `globalThis.g` the program's `A`. (Was: ⊤,
/// "globalThis.Object written by unmodeled code".)
#[test]
fn a_global_read_stays_known_after_an_unknown_native_call() {
    const READ_GT_OBJECT: &str =
        "const O: any = (globalThis as any).Object; console.log(O.keys(new A()).length);";
    let call = "declare function sink(x: any): void;\nsink(1);";
    for src in both_orders(call, &format!("{READ_GT_OBJECT}\n{READ_G}")) {
        let pf = facts(&src);
        assert!(known(&pf), "{src}\n{}", pf.dump_text());
    }
}

/// KEEP: a named store into ⊤ once the global object was handed writes
/// that global: `g` may hold the `B`, `Object` may be replaced.
#[test]
fn a_named_top_store_still_writes_the_handed_global() {
    let hand = "unknown.take2(globalThis); unknown.take(function (o: any) { o.g = new B(); o.Object = 1; });";
    for src in both_orders(hand, &format!("{READ_OBJECT}\n{READ_G}")) {
        let pf = facts(&src);
        assert!(!proven(&pf, "keys"), "{src}\n{}", pf.dump_text());
        assert!(g_sees_b(&pf), "{src}\n{}", pf.dump_text());
    }
}

/// KEEP: a computed store into ⊤ writes every global once the global
/// object was handed.
#[test]
fn a_computed_top_store_still_writes_every_handed_global() {
    let hand = "unknown.take2(globalThis); unknown.take(function (o: any, kk: any) { o[kk] = new B(); });";
    for src in both_orders(hand, &format!("{READ_OBJECT}\n{READ_G}")) {
        let pf = facts(&src);
        assert!(!proven(&pf, "keys"), "{src}\n{}", pf.dump_text());
        assert!(g_sees_b(&pf), "{src}\n{}", pf.dump_text());
    }
}

/// KEEP: the program's own stores of a global keep their values, named
/// and computed, with the global object handed.
#[test]
fn a_program_global_store_keeps_its_value_after_the_global_object_is_handed() {
    for store in [
        "(globalThis as any).g = new B();",
        "(globalThis as any)[process.argv[3]] = new B();",
    ] {
        let hand = format!("unknown.take2(globalThis); {store}");
        for src in both_orders(&hand, READ_G) {
            let pf = facts(&src);
            assert!(g_sees_b(&pf), "{src}\n{}", pf.dump_text());
        }
    }
}

/// KEEP: dynamic code that may run still writes any global by name.
#[test]
fn dynamic_code_still_writes_any_global() {
    for src in both_orders("unknown.take2(globalThis); new Function(process.argv[4])();", READ_OBJECT) {
        let pf = facts_with(&src, interp_cfg());
        assert!(!proven(&pf, "keys"), "{src}\n{}", pf.dump_text());
    }
}
