//! R-GLOBAL (owner decision 2026-11-18, closed world 2026-11-14): the global object
//! given to unknown code is no write of it under any key. It puts ⊤ in
//! reach of the program's stores into ⊤ (`Model::global_in_top`), each
//! recorded under its own key; the program's own stores of a global keep
//! their values. node: `unknown.take2(globalThis)` with no such store
//! leaves `Object` the intrinsic and `g` the program's `A`.
//!
//! Step 2 (owner decision 2026-11-18, closed world 11-14): unknown code
//! that holds the realm (any unknown call) replaces no key of the global
//! object either (`realm_replaced`), `globalThis` included. Step 3 (owner
//! decision 2026-11-18): nor any member of an intrinsic namespace.

use super::*;

const PRE: &str = r#"
    class A { zz = 1; }
    class B { zz = "b"; yy = 2; }
    declare function hostValue(): any; const unknown: any = hostValue;
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
    sites(pf, "zz").iter().any(|s| {
        s.top
            || s.receivers
                .iter()
                .any(|r| r.ends_with(":B") || r.contains(":B#"))
    })
}

fn both_orders(hand: &str, body: &str) -> [String; 2] {
    [
        format!("{PRE}{hand}\n{body}"),
        format!("{PRE}{body}\n{hand}"),
    ]
}

/// No site through the read is ⊤: `O` is the intrinsic `Object`, `g` the
/// program's `A`.
fn known(pf: &ProgramFacts) -> bool {
    sites(pf, "keys").iter().all(|s| !s.top)
        && sites(pf, "zz").iter().all(|s| !s.top)
        && !g_sees_b(pf)
}

/// RED: the global object handed, no store into ⊤: `Object` stays the
/// intrinsic, `g` the program's `A`. (Was: ⊤, the escape recorded an
/// any-key unmodeled write of the global object, and the call of unknown
/// code holding the realm replaced every key of it, `realm_replaced`.)
#[test]
fn a_global_read_stays_known_after_the_global_object_is_handed() {
    for src in both_orders(
        "unknown.take2(globalThis);",
        &format!("{READ_OBJECT}\n{READ_G}"),
    ) {
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

/// RED (step 3): a call of an unknown native holds the realm; it replaces
/// no member of an intrinsic namespace (owner decision 2026-11-18): `M.max`
/// stays the realm's function. (Was: the realm's function or the opaque
/// member, `realm_replaced`'s namespace clause.) `Math.max(..)` itself is
/// the compiled operation; read it off a local.
#[test]
fn a_namespace_member_stays_the_realm_s_after_an_unknown_native_call() {
    let call = "declare function sink(x: any): void;\nsink(1);";
    let body = "const M: any = Math; console.log(M.max(1, 2));";
    for src in both_orders(call, body) {
        let pf = facts(&src);
        assert!(
            sites(&pf, "max")
                .iter()
                .all(|s| !s.top
                    && matches!(s.verdict, Verdict::NativeMember | Verdict::IntrinsicCall)),
            "{src}\n{}",
            pf.dump_text()
        );
    }
}

/// KEEP (step 3 covers namespaces only): a built-in prototype's member read
/// with the realm held stays refused: unknown code may replace it.
#[test]
fn a_builtin_prototype_member_stays_refused_after_an_unknown_native_call() {
    let call = "declare function sink(x: any): void;\nsink(1);";
    let body = "const P: any = Array.prototype; console.log(P.xq);";
    for src in both_orders(call, body) {
        let pf = facts(&src);
        assert!(
            sites(&pf, "xq")
                .iter()
                .all(|s| matches!(s.verdict, Verdict::No(_))),
            "{src}\n{}",
            pf.dump_text()
        );
    }
}

/// KEEP (step 3): the program's own store into a namespace stays an exact
/// write of it with the realm held: `Math.gq` reads the program's `B`.
#[test]
fn a_program_namespace_store_keeps_its_value_with_the_realm_held() {
    let call = "declare function sink(x: any): void;\nsink(1);";
    let body = "(Math as any).gq = new B(); console.log((Math as any).gq.yy);";
    for src in both_orders(call, body) {
        let pf = facts(&src);
        let s = sites(&pf, "yy");
        assert!(
            s.iter().all(|s| !s.top)
                && s.iter().any(|s| s
                    .receivers
                    .iter()
                    .any(|r| r.ends_with(":B") || r.contains(":B#"))),
            "{src}\n{}",
            pf.dump_text()
        );
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
    let hand =
        "unknown.take2(globalThis); unknown.take(function (o: any, kk: any) { o[kk] = new B(); });";
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
    for src in both_orders(
        "unknown.take2(globalThis); new Function(process.argv[4])();",
        READ_OBJECT,
    ) {
        let pf = facts_with(&src, interp_cfg());
        assert!(!proven(&pf, "keys"), "{src}\n{}", pf.dump_text());
    }
}

/// RED (closed world, owner 2026-11-14; R-GLOBAL 2026-11-18): whole program,
/// the interpreter linked (a member named `Function`), the Function
/// constructor given to unknown code, or reached by unknown code that holds
/// the realm (`Function.prototype.constructor`), runs no interpreted code:
/// the code that holds it is the program's or the runtime's, and neither
/// builds a function from source. `globalThis.Object` stays the intrinsic
/// and `g` the program's `A`. (Was: the escape of the constructor ran
/// interpreted code, an any-key write of every global.)
#[test]
fn a_global_read_stays_known_when_unknown_code_holds_the_function_constructor() {
    let read = "const O: any = (globalThis as any).Object; console.log(O.keys(new A()).length);";
    for hand in [
        "unknown.take2(Function);",
        "declare function sink(x: any): void;\nsink(1);",
    ] {
        for src in both_orders(hand, &format!("{read}\n{READ_G}")) {
            let pf = facts_with(&src, interp_cfg());
            assert!(known(&pf), "{src}\n{}", pf.dump_text());
        }
    }
}

/// KEEP: a real source site of interpreted code still writes any global,
/// beside the constructor handed to unknown code.
#[test]
fn a_source_site_beside_the_handed_constructor_still_writes_any_global() {
    let hand = "unknown.take2(Function); new Function(process.argv[4])();";
    for src in both_orders(hand, READ_OBJECT) {
        let pf = facts_with(&src, interp_cfg());
        assert!(!proven(&pf, "keys"), "{src}\n{}", pf.dump_text());
    }
}
