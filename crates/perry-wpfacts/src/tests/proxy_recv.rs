//! Rule PXRECV: a Proxy's forward reads and writes its target with the
//! original Receiver (10.5.8 step 6, 10.5.9 step 6), and an ordinary [[Set]]
//! whose Receiver is a Proxy runs its `getOwnPropertyDescriptor` and
//! `defineProperty` traps (10.1.9.2 OrdinarySetWithOwnDescriptor steps
//! 2.c-e, 10.5.5, 10.5.6).
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

const SET_OUT: &str = "set(t: any, k: any, v: any, r: any) { external(v); return true; }";
const SET_KEEP: &str = "set(t: any, k: any, v: any, r: any) { return true; }";

/// A getter of the target runs with `this` = the Proxy: its store
/// `this.q = payload` is px.[[Set]], whose `set` trap hands the value out.
/// Before PXRECV the getter ran with `this` = the target and the store
/// reached the target only.
#[test]
fn proxy_forward_getter_runs_with_the_proxy_as_this() {
    let t = "{ get x() { (this as any).q = payload; return 1; } }";
    site(t, SET_OUT, "px.x;", false).unwrap();
    site(t, SET_KEEP, "px.x;", true).unwrap();
    // Not read: the getter does not run.
    site(t, SET_OUT, "", true).unwrap();
}

/// A setter of the target runs with `this` = the Proxy: its read
/// `this.z` is px.[[Get]], whose `get` trap hands `payload` out.
#[test]
fn proxy_forward_setter_runs_with_the_proxy_as_this() {
    let t = "{ set y(v: any) { (this as any).z; } }";
    let h = "get(t: any, k: any, r: any) { external(payload); return 1; }";
    site(t, h, "px.y = 1;", false).unwrap();
    site(t, h, "", true).unwrap();
}

/// A Proxy target is passed the original Receiver: the inner `get` trap's
/// `r` is the outer Proxy, whose `set` trap sees `r.q = payload`.
#[test]
fn proxy_forward_to_a_proxy_passes_the_original_receiver() {
    let t = "new Proxy({}, { get(t: any, k: any, r: any) { r.q = payload; return 1; } })";
    site(t, SET_OUT, "px.x;", false).unwrap();
    site(t, SET_KEEP, "px.x;", true).unwrap();
}
