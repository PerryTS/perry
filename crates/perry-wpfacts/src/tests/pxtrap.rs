//! Rule PXTRAP: rules whose operation runs an internal method of a Proxy
//! operand take the PX trap models (the trap read off the handler, called
//! with the spec arguments, and the forward to the target) instead of the
//! PXESC escape. Per rule: the operation on `px` keeps `payload` proven when
//! no trap hands it on (RED on base: the escape hands the target to unknown
//! code); a trap that hands `payload` to unknown code makes it unproven (the
//! trap runs); a handler handed to unknown code keeps it unproven (control).
use super::*;

const PRE: &str = r#"
declare function external(x: unknown): void;
class Payload { n = 1; }
const payload = new Payload();
"#;

/// `hdecl` declares `h`, then `px` = `new Proxy(t, h)`, then `op`; is the
/// site `payload.n` proven? No Proxy part may be held (E9).
fn proven_with(t: &str, hdecl: &str, op: &str) -> Result<bool, String> {
    let src = format!(
        "{PRE}{hdecl}\nconst px: any = new Proxy({t}, h);\n{op}\nconsole.log(payload.n); // LOOKUP_N\n"
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
    if d.contains("E9 Proxy") {
        return Err(format!("a Proxy part is held (E9):\n{src}\n{d}"));
    }
    let p = sites.iter().all(|s| !matches!(s.verdict, Verdict::No(_)));
    if !p && d.contains("(no Proxy model)") {
        return Err(format!("PXESC escape:\n{src}\n{d}"));
    }
    Ok(p)
}

fn expect(t: &str, hdecl: &str, op: &str, want: bool) {
    match proven_with(t, hdecl, op) {
        Ok(p) if p == want => {}
        Ok(p) => panic!("LOOKUP_N proven={p}, expected {want}\nt={t}\nh={hdecl}\nop={op}"),
        Err(e) => panic!("{e}"),
    }
}

/// The three cases of one rule: `t_keep` holds `payload`, no trap; `trap`
/// is a handler whose trap hands `payload` to unknown code (over a target
/// without it); the control hands the handler of `t_keep` to unknown code.
fn pxtrap(t_keep: &str, t_trap: &str, trap: &str, op: &str) {
    expect(t_keep, "const h: any = {};", op, true);
    expect(t_trap, &format!("const h: any = {{ {trap} }};"), op, false);
    expect(t_keep, "const h: any = {};\nexternal(h);", op, false);
}

const GET_OUT: &str = "get(t: any, k: any) { external(payload); return undefined; }";

/// Fulfil: resolving a promise with the Proxy reads its `then`
/// (27.2.1.3.2 step 9): p.[[Get]]( "then" ), the get trap.
#[test]
fn pxtrap_fulfil_reads_then_through_the_get_trap() {
    pxtrap(
        "{ a: payload }",
        "{ a: 1 }",
        GET_OUT,
        "new Promise((res: any) => res(px)).then((v: any) => console.log(typeof v));",
    );
}

/// DefineDesc: a Proxy descriptor of `Object.defineProperties` is read by
/// ToPropertyDescriptor (6.2.6.5): has and get of its six fields.
#[test]
fn pxtrap_define_desc_runs_has_and_get_traps() {
    let op = "const o: any = Object.defineProperties({}, { a: px });\nconsole.log(typeof o.a);";
    pxtrap("{ value: 1, x: payload }", "{ value: 1 }", GET_OUT, op);
    expect(
        "{ value: 1 }",
        "const h: any = { has(t: any, k: any) { external(payload); return k in t; } };",
        op,
        false,
    );
}

/// EnumLoad / EnumKeyStore: a for-in copy of a Proxy subject: its
/// enumeration traps (ownKeys, getOwnPropertyDescriptor, getPrototypeOf)
/// and the get trap of `px[k]`.
#[test]
fn pxtrap_forin_copy_of_a_proxy_runs_its_traps() {
    let op = "const c: any = {};\nfor (const k in px) c[k] = px[k];\nconsole.log(typeof c);";
    pxtrap("{ a: payload }", "{ a: 1 }", GET_OUT, op);
    expect(
        "{ a: 1 }",
        "const h: any = { ownKeys(t: any) { external(payload); return Reflect.ownKeys(t); } };",
        op,
        false,
    );
}

/// Accessor: a Proxy as a getter: the read calls it (its [[Call]]: the
/// apply trap or the forward to its target).
#[test]
fn pxtrap_proxy_getter_is_called_by_the_read() {
    let op = "const o: any = {};\nObject.defineProperty(o, \"a\", { get: px });\nconsole.log(typeof o.a);";
    pxtrap(
        "function () { return payload; }",
        "function () { return 1; }",
        "apply(t: any, th: any, args: any) { external(payload); return 1; }",
        op,
    );
}

const GOPD_OUT: &str =
    "getOwnPropertyDescriptor(t: any, k: any) { external(payload); return undefined; }";
const PROTO_OUT: &str = "getPrototypeOf(t: any) { external(payload); return null; }";
const DEFINE_OUT: &str = "defineProperty(t: any, k: any, d: any) { external(payload); return true; }";

/// Rule PXNT: `Object.prototype`'s methods read under a computed key
/// (`BuiltinNs::ObjectProtoMethod`) called with a Proxy `this` are each
/// method on the Proxy: `hasOwnProperty` its getOwnPropertyDescriptor trap,
/// `toString` the get trap of @@toStringTag, `__lookupGetter__` the
/// getOwnPropertyDescriptor and getPrototypeOf traps (RED on base: PXESC of
/// rule nativethis).
const NT_OP: &str = "const s: string = String(Math.random());\nconst f: any = ({} as any)[s];\nconsole.log(f.call(px, \"a\"));";

#[test]
fn pxnt_object_proto_methods_run_the_gopd_trap() {
    pxtrap("{ a: payload }", "{ a: 1 }", GOPD_OUT, NT_OP);
}

#[test]
fn pxnt_object_proto_methods_run_the_get_trap() {
    pxtrap("{ a: payload }", "{ a: 1 }", GET_OUT, NT_OP);
}

#[test]
fn pxnt_object_proto_methods_run_the_getprototypeof_trap() {
    pxtrap("{ a: payload }", "{ a: 1 }", PROTO_OUT, NT_OP);
}

/// Rule PXANNEXB: the Annex B define on a Proxy receiver
/// (`js_object_define_getter`, `Cons::AnnexBDefine`; B.2.2.2) is its
/// [[DefineOwnProperty]]: the defineProperty trap, passed a descriptor
/// whose `get` is the getter (RED on base: PXESC of rule annexbdefine).
const XDECL: &str = "declare function js_object_define_getter(o: any, k: any, f: any): any;\ndeclare function js_object_lookup_getter(o: any, k: any): any;\n";

#[test]
fn pxannexb_define_runs_the_define_property_trap() {
    let op = format!("{XDECL}js_object_define_getter(px, \"g\", function (this: any) {{ return 1; }});");
    pxtrap("{ a: payload }", "{ a: 1 }", DEFINE_OUT, &op);
}

#[test]
fn pxannexb_define_passes_the_getter_in_the_descriptor() {
    let op = format!("{XDECL}js_object_define_getter(px, \"g\", function (this: any) {{ return payload; }});");
    expect("{ a: 1 }", "const h: any = {};", &op, true);
    let trap = "const h: any = { defineProperty(t: any, k: any, d: any) { external(d.get); return true; } };";
    expect("{ a: 1 }", trap, &op, false);
}

/// Rule PXANNEXB: the Annex B lookup on a Proxy receiver
/// (`js_object_lookup_getter`, `Cons::AnnexBLookup`; B.2.2.4) runs its
/// getOwnPropertyDescriptor and getPrototypeOf traps (RED on base: PXESC of
/// rule annexblookup).
#[test]
fn pxannexb_lookup_runs_the_gopd_trap() {
    let op = format!("{XDECL}console.log(js_object_lookup_getter(px, \"a\"));");
    pxtrap("{ a: payload }", "{ a: 1 }", GOPD_OUT, &op);
}

#[test]
fn pxannexb_lookup_runs_the_getprototypeof_trap() {
    let op = format!("{XDECL}console.log(js_object_lookup_getter(px, \"a\"));");
    pxtrap("{ a: payload }", "{ a: 1 }", PROTO_OUT, &op);
}
