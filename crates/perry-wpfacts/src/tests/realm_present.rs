//! REALM-PRESENT: a declared realm global (ECMA-262 9.1.1.4: the global
//! environment's object record is the global object, whose own properties
//! the realm creates, 19.1-19.4) or a declared own member of a builtin
//! namespace or constructor (`Math.max`, 21.3.2) is present: a read
//! (10.1.8.1 OrdinaryGet) finds the realm's value and never `undefined`,
//! unless a write or `delete` can reach that key (both are writable and
//! configurable, 10.4.1.2 / 18): a program store or delete under the key,
//! a write under an unknown key, the object handed to unknown code. Those
//! keep today's answer (`undefined` or ⊤ beside the realm value).
//! three.js skin: `{ then: Int8Array }` as a fulfilment value: the
//! `undefined` of the `then` read let the fulfil guard pass the thenable
//! itself on as the fulfilment value (27.2.1.3.2 step 9-13).
use super::*;

const UNDEF: u16 = 0x10;

/// What `x` (stored into `holder.xv`) may hold after `body`: (prims, ⊤, objs).
fn xv(body: &str) -> (u16, bool, Vec<String>) {
    let src = format!(
        r#"
        const k: any = process.argv[2];
        const holder: any = {{ xv: 0 }};
        {body}
        console.log(holder.xv);
        "#
    );
    let pf = facts(&src);
    let fs: Vec<_> = pf.templates.iter().flat_map(|t| &t.fields).filter(|f| f.key == "xv").collect();
    assert!(!fs.is_empty(), "no field xv");
    let mut p = 0u16;
    let mut top = false;
    let mut objs = vec![];
    for f in fs {
        p |= f.kind.prims;
        top |= f.kind.top;
        objs.extend(f.kind.objs.iter().cloned());
    }
    (p, top, objs)
}

fn maybe_undef(body: &str) -> bool {
    let (p, top, _) = xv(body);
    top || p & UNDEF != 0
}

/// The declared reads: never `undefined`.
#[test]
fn a_declared_realm_global_or_namespace_member_is_present() {
    let bad: Vec<&str> = [
        "const x: any = Int8Array; holder.xv = x;",
        "const x: any = Map; holder.xv = x;",
        "const x: any = Math.max; holder.xv = x;",
        "const x: any = Array; holder.xv = x;",
        "const x: any = Object; holder.xv = x;",
        "const x: any = (globalThis as any).Int8Array; holder.xv = x;",
    ]
    .into_iter()
    .filter(|b| maybe_undef(b))
    .collect();
    assert!(bad.is_empty(), "read as possibly absent: {bad:#?}");
}

/// Controls: a write or delete that can reach the key keeps `undefined`
/// (or ⊤); an undeclared global keeps today's answer.
#[test]
fn a_reachable_write_or_delete_keeps_undefined() {
    let bad: Vec<&str> = [
        "delete (globalThis as any).Int8Array; const x: any = Int8Array; holder.xv = x;",
        "(Math as any).max = undefined; const x: any = Math.max; holder.xv = x;",
        "delete (Math as any).max; const x: any = Math.max; holder.xv = x;",
        "(globalThis as any)[k] = undefined; const x: any = Int8Array; holder.xv = x;",
        "(globalThis as any).hostGet()(globalThis); const x: any = Int8Array; holder.xv = x;",
        "const x: any = (globalThis as any).someUndeclared; holder.xv = x;",
    ]
    .into_iter()
    .filter(|b| !maybe_undef(b))
    .collect();
    assert!(bad.is_empty(), "lost undefined: {bad:#?}");
}

/// The skin shape: a builtin-valued `then` is callable, so the thenable is
/// never the fulfilment value; `then: 5` is not callable, so it is.
#[test]
fn a_builtin_then_never_fulfils_with_the_thenable() {
    let f = |t: &str| {
        let b = format!("const t: any = {{ then: {t}, zz: 1 }}; Promise.resolve(t).then((x: any) => {{ holder.xv = x; }}, () => 0);");
        !xv(&b).2.is_empty()
    };
    let bad: Vec<&str> = ["Int8Array", "Map", "Math.max", "Array", "Object"].into_iter().filter(|t| f(t)).collect();
    assert!(bad.is_empty(), "fulfilled with the thenable: {bad:#?}");
    assert!(f("5"), "then: 5 must fulfil with the thenable");
}

