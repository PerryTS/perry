//! Rule PXFULL: the Proxy operations that used to degrade (E9 target and
//! handler sink) have their exact ECMA-262 10.5 models: definitions
//! (PXDEFP, 10.5.6).
use super::*;

const PRE: &str = r#"
declare function external(x: unknown): void;
class Payload { n = 1; }
const payload = new Payload();
"#;

/// `px` = `new Proxy(t, h)`, then `op`; the site `payload.n` is proven
/// unless `payload` reached unknown code. No Proxy part is held (E9).
fn site(t: &str, h: &str, op: &str, proven: bool) -> Result<(), String> {
    let src = format!(
        "{PRE}const px: any = new Proxy({t}, {{ {h} }});\n{op}\nconsole.log(payload.n); // LOOKUP_N\n"
    );
    let at = src.find("// LOOKUP_N").expect("named site");
    let line = src[..at].rfind('\n').map_or(0, |p| p + 1) as u32;
    let pf = facts(&src);
    let d = pf.dump_text();
    let sites: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| !s.is_call && s.key == "n" && (line..at as u32).contains(&s.byte_offset))
        .collect();
    if sites.is_empty() {
        return Err(format!("missing LOOKUP_N\n{d}"));
    }
    if sites
        .iter()
        .any(|s| !matches!(s.verdict, Verdict::No(_)) != proven)
    {
        return Err(format!(
            "LOOKUP_N expected proven={proven}: {:?}\n{src}\n{d}",
            sites.iter().map(|s| &s.verdict).collect::<Vec<_>>()
        ));
    }
    if d.contains("E9 Proxy") {
        return Err(format!("a Proxy part is held (E9):\n{src}\n{d}"));
    }
    Ok(())
}

const DEF_OUT: &str = "defineProperty(t: any, k: any, d: any) { external(payload); return true; }";
const DEF_KEEP: &str = "defineProperty(t: any, k: any, d: any) { return true; }";
const DEF_VALUE: &str = "defineProperty(t: any, k: any, d: any) { external(d.value); return true; }";
const DEF_GET: &str = "defineProperty(t: any, k: any, d: any) { external(d.get()); return true; }";
const SET_OUT: &str = "set(t: any, k: any, v: any, r: any) { external(payload); return true; }";

/// Object.defineProperty on a Proxy runs its `defineProperty` trap.
#[test]
fn proxy_define_property_runs_the_trap() {
    let op = "Object.defineProperty(px, \"a\", { value: 1 });";
    site("{}", DEF_OUT, op, false).unwrap();
    site("{}", DEF_KEEP, op, true).unwrap();
    site("{}", DEF_OUT, "", true).unwrap();
}

/// The trap is passed a descriptor object whose `value` is the defined one.
#[test]
fn proxy_define_property_trap_sees_the_value() {
    site("{}", DEF_VALUE, "Object.defineProperty(px, \"a\", { value: payload });", false).unwrap();
    site("{}", DEF_VALUE, "Object.defineProperty(px, \"a\", { value: 1 });", true).unwrap();
}

/// An accessor descriptor: the trap's descriptor `get` is the getter.
#[test]
fn proxy_define_property_trap_sees_the_getter() {
    let op = "Object.defineProperty(px, \"a\", { get() { return payload; } });";
    site("{}", DEF_GET, op, false).unwrap();
    let op1 = "Object.defineProperty(px, \"a\", { get() { return 1; } });";
    site("{}", DEF_GET, op1, true).unwrap();
}

/// A definition is no [[Set]]: the `set` trap and the target's setter do
/// not run.
#[test]
fn proxy_define_property_runs_no_set() {
    let op = "Object.defineProperty(px, \"a\", { value: 1 });";
    site("{}", SET_OUT, op, true).unwrap();
    let t = "{ set a(v: any) { external(payload); } }";
    site(t, "", op, true).unwrap();
}

/// Without a trap the definition forwards to the target: its value is the
/// target's, which a later read of the Proxy forwards to.
#[test]
fn proxy_define_property_forwards_the_value_and_the_getter() {
    let op = "Object.defineProperty(px, \"a\", { value: payload }); external(px.a);";
    site("{}", "", op, false).unwrap();
    let op = "Object.defineProperty(px, \"a\", { value: 1 }); external(px.a);";
    site("{}", "", op, true).unwrap();
    let op = "Object.defineProperty(px, \"a\", { get() { return payload; } }); external(px.a);";
    site("{}", "", op, false).unwrap();
}

/// Reflect.defineProperty and Object.defineProperties run the same trap.
#[test]
fn proxy_reflect_define_and_define_properties_run_the_trap() {
    site("{}", DEF_VALUE, "Reflect.defineProperty(px, \"a\", { value: payload });", false).unwrap();
    site("{}", DEF_VALUE, "Reflect.defineProperty(px, \"a\", { value: 1 });", true).unwrap();
    site("{}", DEF_VALUE, "Object.defineProperties(px, { a: { value: payload } });", false).unwrap();
    site("{}", DEF_VALUE, "Object.defineProperties(px, { a: { value: 1 } });", true).unwrap();
}

/// A computed key: the trap runs under any key.
#[test]
fn proxy_define_property_computed_key_runs_the_trap() {
    let op = "Object.defineProperty(px, String(Math.random()), { value: payload });";
    site("{}", DEF_VALUE, op, false).unwrap();
    let op = "Object.defineProperty(px, String(Math.random()), { value: 1 });";
    site("{}", DEF_VALUE, op, true).unwrap();
}

const FN: &str = "function (this: any, x: any) { return 1; }";
const APPLY_OUT: &str = "apply(t: any, th: any, a: any) { external(payload); return 1; }";
const APPLY_ARG: &str = "apply(t: any, th: any, a: any) { external(a[0]); return 1; }";
const APPLY_THIS: &str = "apply(t: any, th: any, a: any) { external(th.q); return 1; }";

/// A call of a callable Proxy runs its `apply` trap (10.5.12).
#[test]
fn proxy_call_runs_the_apply_trap() {
    site(FN, APPLY_OUT, "px();", false).unwrap();
    site(FN, APPLY_OUT, "", true).unwrap();
    site(FN, APPLY_ARG, "px(payload);", false).unwrap();
    site(FN, APPLY_ARG, "px(1);", true).unwrap();
    site(FN, APPLY_ARG, "px(...[payload]);", false).unwrap();
    site(FN, APPLY_THIS, "({ m: px, q: payload }).m();", false).unwrap();
    site(FN, APPLY_THIS, "({ m: px, q: 1 }).m();", true).unwrap();
}

/// A Proxy whose target is not callable has no [[Call]]: no trap runs.
#[test]
fn proxy_call_of_a_non_callable_target_runs_no_trap() {
    site("{}", APPLY_OUT, "try { px(); } catch (e) {}", true).unwrap();
}

/// Without an `apply` trap the call forwards to the target with the same
/// `this` and arguments; its result is the call's.
#[test]
fn proxy_call_forwards_to_the_target() {
    let t = "function (this: any, x: any) { external(x); }";
    site(t, "", "px(payload);", false).unwrap();
    site(t, "", "px(1);", true).unwrap();
    let t = "function (this: any) { external(this.q); }";
    site(t, "", "({ m: px, q: payload }).m();", false).unwrap();
    site(t, "", "({ m: px, q: 1 }).m();", true).unwrap();
    let t = "function () { return payload; }";
    site(t, "", "external(px());", false).unwrap();
}

/// `new` of a Proxy runs its `construct` trap (10.5.13); only an object
/// result is the `new`'s.
#[test]
fn proxy_new_runs_the_construct_trap() {
    let t = "class { constructor(x: any) {} }";
    let h = "construct(t: any, a: any, nt: any) { external(a[0]); return {}; }";
    site(t, h, "new px(payload);", false).unwrap();
    site(t, h, "new px(1);", true).unwrap();
    let h = "construct(t: any, a: any, nt: any) { return payload; }";
    site(t, h, "external(new px());", false).unwrap();
    let h = "construct(t: any, a: any, nt: any) { return { k: 1 }; }";
    site(t, h, "external(new px());", true).unwrap();
}

/// Without a `construct` trap `new` forwards to the target.
#[test]
fn proxy_new_forwards_to_the_target() {
    let t = "class { constructor(x: any) { external(x); } }";
    site(t, "", "new px(payload);", false).unwrap();
    site(t, "", "new px(1);", true).unwrap();
}

/// `new` without a `construct` trap: GetPrototypeFromConstructor reads
/// `prototype` through the Proxy (its get trap); a primitive there gives
/// the new object the realm's default prototype, `%Object.prototype%`
/// (10.1.14 step 4), whose program method `f` then runs.
#[test]
fn proxy_new_primitive_prototype_gives_the_realm_default() {
    let t = "(() => { function T(this: any) {} T.prototype = Object.create(null); return T; })()";
    let f = "(Object.prototype as any).f = function () { external(payload); };\n";
    let h = "get(t: any, k: any) { return 1; }";
    site(t, h, &format!("{f}const o: any = new px(); o.f();"), false).unwrap();
}

/// Rule PXPROTO: `Object.setPrototypeOf( p, V )` runs the `setPrototypeOf`
/// trap with V (10.5.2) and forwards to the target: a [[Get]] forwarded
/// to the target then finds V's members.
#[test]
fn proxy_set_prototype_of_runs_the_trap_and_forwards() {
    let h = "setPrototypeOf(t: any, v: any) { external(v.q); return true; }";
    site("{}", h, "Object.setPrototypeOf(px, { q: payload });", false).unwrap();
    site("{}", h, "Object.setPrototypeOf(px, { q: 1 });", true).unwrap();
    let op = "Object.setPrototypeOf(px, { m() { external(payload); } }); px.m();";
    site("{}", "", op, false).unwrap();
    let op = "Object.setPrototypeOf(px, { m() { return 1; } }); px.m();";
    site("{}", "", op, true).unwrap();
}

/// Rule PXDEL: `delete p[k]` under a computed key runs the
/// `deleteProperty` trap (10.5.10) and forwards to the target.
#[test]
fn proxy_delete_computed_key_runs_the_trap() {
    let op = "delete px[String(Math.random())];";
    let h = "deleteProperty(t: any, k: any) { external(payload); return true; }";
    site("{}", h, op, false).unwrap();
    site("{}", "deleteProperty(t: any, k: any) { return true; }", op, true).unwrap();
    site("{}", "", op, true).unwrap();
}

const PE_TRAP: &str = "preventExtensions(t: any) { external(payload); return true; }";
const KEYS_TRAP: &str = "ownKeys(t: any) { external(payload); return []; }";
const GOPD_TRAP: &str = "getOwnPropertyDescriptor(t: any, k: any) { external(payload); return undefined; }";
const DEF_TRAP: &str = "defineProperty(t: any, k: any, d: any) { external(payload); return true; }";

/// Rule PXINT: each integrity level runs exactly its traps (10.5.4,
/// 7.3.15): preventExtensions the `preventExtensions` trap; seal also
/// `ownKeys` and `defineProperty`; freeze also `getOwnPropertyDescriptor`.
#[test]
fn proxy_integrity_levels_run_their_traps() {
    let t = "{ a: 1 }";
    for (op, pe, keys, gopd, def) in [
        ("Object.preventExtensions(px);", true, false, false, false),
        ("Reflect.preventExtensions(px);", true, false, false, false),
        ("Object.seal(px);", true, true, false, true),
        ("Object.freeze(px);", true, true, true, true),
    ] {
        for (h, runs) in [(PE_TRAP, pe), (KEYS_TRAP, keys), (GOPD_TRAP, gopd), (DEF_TRAP, def)] {
            site(t, h, op, !runs).unwrap_or_else(|e| panic!("{op} {h}: {e}"));
        }
    }
    site(t, "", "Object.freeze(px);", true).unwrap();
}
