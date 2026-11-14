//! Rule D1 fixture (three.js Object3D constructor, three.core.js:
//! `Object.defineProperty( this, "id", { value: _object3DId ++ } )`, then
//! `this.uuid = generateUUID()`; `bindingsByRoot[ root.uuid ]` at 55213).
//! ECMA-262 20.1.2.4 -> 7.3.8 -> 10.1.6.3: a data descriptor under the named
//! key P creates or changes P alone, to [[Value]]: no other key is written,
//! no accessor is installed. The object's other keys stay as they were
//! (NAMED-DATA-DEFINE: not HARD, the defined key's own sites stay open), and
//! its any-key field gets no "any value" seed from such a definition; any other HARD mark still seeds it, in either
//! order. Controls: an unknown key, a delete under an unknown key (after
//! and before the definition), an unknown descriptor, a named accessor;
//! a descriptor from `JSON.parse` defines data.
use super::*;
use crate::model::prim;

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

/// The primitive kinds the receivers of the `key` sites hold.
fn prims(pf: &ProgramFacts, key: &str) -> u16 {
    pf.sites
        .iter()
        .filter(|s| s.key == key)
        .fold(0, |a, s| a | s.receiver_prims)
}

/// `pre` runs in the constructor before the definition `def`, `post` after
/// the instance is built.
fn prog(pre: &str, def: &str, post: &str) -> String {
    format!(
        "class B {{ p = 1; }}
         let n = 0;
         const lut: string[] = [];
         for (let i = 0; i < 256; i++) lut[i] = (i < 16 ? '0' : '') + i.toString(16);
         function generateUUID(): string {{
           const d0 = Math.random() * 0xffffffff | 0;
           const d1 = Math.random() * 0xffffffff | 0;
           const uuid = lut[d0 & 0xff] + lut[d0 >> 8 & 0xff] + '-' + lut[d1 & 0xff] + lut[d1 >> 8 & 0xff];
           return uuid.toLowerCase();
         }}
         class A {{ uuid: string; constructor() {{ {pre} {def}; this.uuid = generateUUID(); }} }}
         const a = new A();
         {post}
         const byRoot: any = {{}};
         let m: any = byRoot[a.uuid];
         if (m === undefined) {{ m = new B(); byRoot[a.uuid] = m; }}
         const x: any = byRoot[a.uuid];
         console.log(m.p, x.p);
         const u: any = a.uuid; console.log(u.length);
         const i: any = (a as any).id; console.log(i.toFixed(1));"
    )
}

const NAMED: &str = "Object.defineProperty(this, \"id\", { value: n++ })";

#[test]
fn named_data_define_reads_no_builtin_member() {
    let v = recv(&facts(&prog("", NAMED, "")));
    assert_eq!(v, vec!["B"], "{v:?}");
}

#[test]
fn named_data_define_keeps_field_values() {
    let pf = facts(&prog("", NAMED, ""));
    // `uuid: string;` is a class field, undefined from birth until the
    // constructor's store; the definition neither leaks `this` nor runs
    // program code, and every read of `a.uuid` follows `new A()`: STR alone.
    assert_eq!(prims(&pf, "length"), prim::STR, "uuid");
    // The constructor defines `id` before any program code runs on `this`,
    // in birth order (the family layout's `Defined` slot): every read of
    // `a.id` after `new A()` sees the number `n++`, never undefined.
    assert_eq!(prims(&pf, "toFixed"), prim::NUM, "id");
}

#[test]
fn unknown_key_define_seeds() {
    let def = "Object.defineProperty(this, process.argv[2], { value: 1 })";
    let v = recv(&facts(&prog("", def, "")));
    assert!(v.len() > 1, "{v:?}");
}

#[test]
fn later_hard_mark_seeds() {
    let v = recv(&facts(&prog("", NAMED, "delete (a as any)[process.argv[2]];")));
    assert!(v.len() > 1, "{v:?}");
}

#[test]
fn earlier_hard_mark_seeds() {
    let v = recv(&facts(&prog("delete (this as any)[process.argv[2]];", NAMED, "")));
    assert!(v.len() > 1, "{v:?}");
}

#[test]
fn unknown_descriptor_seeds() {
    let def = "Object.defineProperty(this, \"id\", (globalThis as any).desc)";
    let v = recv(&facts(&unseen(&prog("", def, ""))));
    assert!(v.len() > 1, "{v:?}");
}

/// A descriptor from `JSON.parse` holds no function: a get/set it names
/// throws (ECMA-262 6.2.6.5 steps 11-14), so it defines data.
#[test]
fn json_descriptor_defines_data() {
    let def = "Object.defineProperty(this, \"id\", JSON.parse(process.argv[2]))";
    let v = recv(&facts(&prog("", def, "")));
    assert_eq!(v, vec!["B"], "{v:?}");
}

#[test]
fn named_accessor_define_as_today() {
    let def = "Object.defineProperty(this, \"id\", { get() { return 1; } })";
    let v = recv(&facts(&prog("", def, "")));
    assert!(v.len() > 1, "{v:?}");
}
