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
