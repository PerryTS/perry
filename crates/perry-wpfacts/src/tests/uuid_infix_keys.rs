//! K7 fixture (three.js `AnimationMixer._bindAction`, three.core.js:55213:
//! `bindingsByRoot[ rootUuid ]` with `rootUuid = root.uuid` from
//! `generateUUID()`): a string built by `+` with a constant `'-'` between
//! its parts (ECMA-262 13.15.3: the constant is part of the result) and then
//! flattened by `String.prototype.toLowerCase` (22.1.3.28: the `-` maps to
//! itself) contains a `-`, so it never names an Object.prototype member (no
//! built-in member name contains `-`). On this head the read reads every
//! Object.prototype method (builtin-chain-any), the binding found there is a
//! built-in method object, and its unlocked fields carry the glTF JSON into
//! `Interpolant::copySampleValue_` (K7). Controls: a key from
//! `process.argv[2]` and `process.argv[2].toLowerCase()` (may be
//! `constructor`), `'to' + process.argv[2]` (may be `toString`).
use super::*;

/// The sorted class names (or other tokens) the `.p` reads see.
fn recv(pf: &ProgramFacts) -> Vec<String> {
    let mut r: Vec<String> = pf
        .sites
        .iter()
        .filter(|s| s.key == "p" && !s.is_call)
        .flat_map(|s| s.receivers.iter())
        .map(|r| r.rsplit(':').next().unwrap_or(r).to_string())
        .collect();
    r.sort();
    r.dedup();
    r
}

fn prog(key: &str) -> String {
    format!(
        "class A {{ p = 1; }}
         const lut: string[] = [];
         for (let i = 0; i < 256; i++) lut[i] = (i < 16 ? '0' : '') + i.toString(16);
         function generateUUID(): string {{
           const d0 = Math.random() * 0xffffffff | 0;
           const d1 = Math.random() * 0xffffffff | 0;
           const uuid = lut[d0 & 0xff] + lut[d0 >> 8 & 0xff] + '-' + lut[d1 & 0xff] + lut[d1 >> 8 & 0xff];
           return uuid.toLowerCase();
         }}
         class Root {{ uuid: string; constructor() {{ this.uuid = generateUUID(); }} }}
         const root = new Root();
         const byRoot: any = {{}};
         const rootUuid = {key};
         let m: any = byRoot[rootUuid];
         if (m === undefined) {{ m = new A(); byRoot[rootUuid] = m; }}
         const x: any = byRoot[rootUuid];
         console.log(m.p, x.p);"
    )
}

#[test]
fn dash_infix_key_reads_no_builtin_member() {
    let v = recv(&facts(&prog("root.uuid")));
    assert_eq!(v, vec!["A"], "{v:?}");
}

#[test]
fn argv_key_reads_builtin_members() {
    let v = recv(&facts(&prog("process.argv[2]")));
    assert!(v.len() > 1, "{v:?}");
}

#[test]
fn argv_lowercase_key_reads_builtin_members() {
    let v = recv(&facts(&prog("String(process.argv[2]).toLowerCase()")));
    assert!(v.len() > 1, "{v:?}");
}

#[test]
fn to_prefix_argv_key_reads_builtin_members() {
    let v = recv(&facts(&prog("'to' + process.argv[2]")));
    assert!(v.len() > 1, "{v:?}");
}

#[test]
fn argv_then_const_suffix_key_reads_builtin_members() {
    // `x + 'a'`: no character outside IdentifierPart is sure.
    let v = recv(&facts(&prog("process.argv[2] + 'a'")));
    assert!(v.len() > 1, "{v:?}");
}

#[test]
fn replace_drops_infix() {
    let v = recv(&facts(&prog("('x-' + process.argv[2]).replace('-', '')")));
    assert!(v.len() > 1, "{v:?}");
}

#[test]
fn slice_drops_infix() {
    let v = recv(&facts(&prog("root.uuid.slice(1)")));
    assert!(v.len() > 1, "{v:?}");
}

#[test]
fn field_joined_with_argv_drops_infix() {
    let src = prog("root.uuid").replace(
        "this.uuid = generateUUID();",
        "this.uuid = generateUUID(); if (process.argv.length > 5) this.uuid = process.argv[2];",
    );
    let v = recv(&facts(&src));
    assert!(v.len() > 1, "{v:?}");
}

// Rule K8 (ECMA-262 7.1.19 ToPropertyKey, 7.1.17 ToString): a key that is a
// dash-infix String or `undefined` / `null` reads the infix names and the
// named key "undefined" / "null"; neither names an Object.prototype member.
// three.js `_bindAction`: `root.uuid` of a root that may lack `uuid`.

#[test]
fn dash_infix_or_absent_uuid_key_reads_no_builtin_member() {
    let v = recv(&facts(&prog(
        "(Math.random() < 0.5 ? root : ({} as any)).uuid",
    )));
    assert_eq!(v, vec!["A"], "{v:?}");
}

#[test]
fn dash_infix_or_null_key_reads_no_builtin_member() {
    let v = recv(&facts(&prog("(Math.random() < 0.5 ? root.uuid : null)")));
    assert_eq!(v, vec!["A"], "{v:?}");
}

#[test]
fn argv_or_undefined_key_reads_builtin_members() {
    let v = recv(&facts(&prog(
        "(Math.random() < 0.5 ? process.argv[2] : undefined)",
    )));
    assert!(v.len() > 1, "{v:?}");
}
