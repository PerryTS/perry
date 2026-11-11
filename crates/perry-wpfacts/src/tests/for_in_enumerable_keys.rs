//! ENUM1: a for-in copy reads only enumerable keys. Inside `for (k in o)`,
//! `o[k]` (the loop's own key on the same, unwritten binding) on a built-in
//! ECMAScript object yields what the program wrote on it (and on its chain),
//! never a built-in member: none of those is enumerable. Any other receiver,
//! and the key on another object, read as before.
use super::*;

fn v(src: &str) -> crate::derive::KindFact {
    let pf = facts_with(src, interp_cfg());
    field(tmpl(&pf, "R"), "v").kind.clone()
}

const R: &str = "class R { v: any; constructor(v: any) { this.v = v; } }\nclass K { x = 1; }\n";
const HAS: &str = "function hasProperty(m: any, k: string): boolean { return Object.prototype.hasOwnProperty.call(m, k); }\n";
const ASSIGN: &str = "function assign(t: any, ...args: any[]): any {\n  for (const arg of args) {\n    if (arg === undefined) continue;\n    for (const p in arg) {\n      if (hasProperty(arg, p)) {\n        t[p] = arg[p];\n      }\n    }\n  }\n  return t;\n}\n";

fn none_builtin(k: &crate::derive::KindFact) -> bool {
    !k.top && k.funcs == 0 && k.builtin_objs == 0
}

/// (a) tsc's `assign` (core.ts), called as commandLineParser's
/// `assign({}, mapLike)` with `Object` merged in, then ToPrimitive of the
/// target: the target must not escape through a copied built-in method.
#[test]
fn tssrc_assign_copy_from_object_adds_no_builtin_member() {
    let src = format!(
        "{R}{HAS}{ASSIGN}const out: any = assign({{ k: new K() }}, Object as any);\nconsole.log([out].join(\",\"), `${{out}}`);\nconst r = new R(out.k);\nconsole.log(r.v.x);\n"
    );
    let k = v(&src);
    eprintln!("a k {k:?}");
    assert!(
        !k.top && k.objs.len() == 1 && k.objs[0].ends_with(":K"),
        "out.k {k:?}"
    );
    let src2 = format!("{R}{HAS}{ASSIGN}const out: any = assign(Object.create(null), Object as any, Math as any);\nconst r = new R(out.zz);\nconsole.log(r.v);\n");
    let z = v(&src2);
    eprintln!("a zz {z:?}");
    assert!(none_builtin(&z), "out.zz {z:?}");
}

fn copy_of(obj: &str) -> String {
    format!("{R}function copy(): any {{ const t: any = Object.create(null); const s: any = {obj}; for (const p in s) {{ t[p] = s[p]; }} return t; }}\nconst r = new R(copy().zz);\nconsole.log(r.v);\n")
}

/// (b) plain forms over built-in objects.
#[test]
fn plain_copies_from_builtins_add_no_member() {
    // Not `Array.prototype` (its value is also `ObjectProto`, which stands
    // for every builtin prototype, host ones among them) and not a builtin
    // function value (a host function may hold enumerable data): unchanged.
    for obj in ["Object", "Math", "JSON", "Reflect", "Map", "Promise"] {
        let k = v(&copy_of(obj));
        eprintln!("b {obj} {k:?}");
        assert!(none_builtin(&k), "{obj}: {k:?}");
    }
}

/// (b) at module top level (a loop that stays a loop in a unit view).
#[test]
fn top_level_copy_from_object_adds_no_member() {
    let src = format!("{R}const t: any = Object.create(null);\nconst s: any = Object;\nfor (const p in s) {{ t[p] = s[p]; }}\nconst r = new R(t.zz);\nconsole.log(r.v);\n");
    let k = v(&src);
    eprintln!("b top {k:?}");
    assert!(none_builtin(&k), "{k:?}");
}

/// (c) what the program wrote on the built-in (own, and on its chain) is
/// copied.
#[test]
fn program_written_keys_on_builtin_are_copied() {
    let own = format!("{R}function f() {{ return 1; }}\n(Object as any).mine = f;\nfunction copy(): any {{ const t: any = Object.create(null); const s: any = Object; for (const p in s) {{ t[p] = s[p]; }} return t; }}\nconst r = new R(copy().mine);\nconsole.log(r.v);\n");
    let k = v(&own);
    eprintln!("c own {k:?}");
    assert!(
        !k.top && k.funcs >= 1 && k.prims != crate::model::prim::ALL,
        "{k:?}"
    );
    let up = format!("{R}function g() {{ return 2; }}\n(Function.prototype as any).up = g;\nfunction copy(): any {{ const t: any = Object.create(null); const s: any = Object; for (const p in s) {{ t[p] = s[p]; }} return t; }}\nconst r = new R(copy().up);\nconsole.log(r.v);\n");
    let k = v(&up);
    eprintln!("c chain {k:?}");
    // A store onto `Function.prototype` leaves Object's chain read open here.
    assert!(k.top || k.funcs >= 1, "{k:?}");
}

/// (d) controls: the full read stays.
#[test]
fn other_reads_keep_the_full_lookup() {
    let has = |k: &crate::derive::KindFact| k.top || k.funcs + k.builtin_objs > 0;
    // The loop key on another object.
    let other = format!("{R}function copy(a: any): any {{ const t: any = {{}}; const b: any = Object; for (const k in a) {{ t[k] = b[k]; }} return t; }}\nconst r = new R(copy({{ toString: 1, keys: 2 }}).keys);\nconsole.log(r.v);\n");
    // The key reassigned in the body.
    let key_w = format!("{R}function copy(): any {{ const t: any = {{}}; const s: any = Object; for (let p in s) {{ p = p + process.argv[2]; t[p] = s[p]; }} return t; }}\nconst r = new R(copy().zz);\nconsole.log(r.v);\n");
    // The iterated binding reassigned in the body.
    let obj_w = format!("{R}function copy(): any {{ const t: any = {{}}; let s: any = Object; for (const p in s) {{ s = Math; t[p] = s[p]; }} return t; }}\nconst r = new R(copy().zz);\nconsole.log(r.v);\n");
    // An unknown key outside any loop.
    let unk = format!("{R}const r = new R((Object as any)[process.argv[2]]);\nconsole.log(r.v);\n");
    for (name, src) in [
        ("other", other),
        ("key_w", key_w),
        ("obj_w", obj_w),
        ("unk", unk),
    ] {
        let k = v(&src);
        eprintln!("d {name} {k:?}");
        assert!(has(&k), "{name}: {k:?}");
    }
    // For-in over a program object reads its fields and inherited fields.
    let prog = format!("{R}function f() {{ return 1; }}\nfunction g() {{ return 2; }}\nfunction copy(): any {{ const t: any = {{}}; const s: any = Object.create({{ inh: f }}); s.own = g; for (const p in s) {{ t[p] = s[p]; }} return t; }}\nconst r = new R(copy().zz);\nconsole.log(r.v);\n");
    let k = v(&prog);
    eprintln!("d prog {k:?}");
    assert!(k.funcs >= 2, "prog {k:?}");
}

/// (e) loops that stay loops, and a copy through composed helpers.
#[test]
fn loop_forms_and_composed_helpers() {
    let ret = format!("{R}function copy(t: any, s: any): any {{ for (const p in s) {{ if (p === \"stop\") return t; t[p] = s[p]; }} return t; }}\nconst r = new R(copy(Object.create(null), Object).zz);\nconsole.log(r.v);\n");
    let brk = format!("{R}function copy(t: any, s: any): any {{ for (const p in s) {{ t[p] = s[p]; if (p === \"stop\") break; }} return t; }}\nconst r = new R(copy(Object.create(null), Math).zz);\nconsole.log(r.v);\n");
    let cap = format!("{R}const fs: any[] = [];\nfunction copy(t: any, s: any): any {{ for (const p in s) {{ fs.push(() => p); t[p] = s[p]; }} return t; }}\nconst r = new R(copy(Object.create(null), Object).zz);\nconsole.log(r.v, fs.length);\n");
    let comp = format!("{R}{HAS}{ASSIGN}function merge(a: any, b: any): any {{ return assign(Object.create(null), a, b); }}\nfunction mergeAll(a: any, b: any): any {{ return merge(a, b); }}\nconst o1: any = mergeAll(Object, Math);\nconst o2: any = mergeAll(JSON, Reflect);\nconst r = new R(o1.zz);\nconsole.log(r.v, o2.zz);\n");
    for (name, src) in [
        ("return", ret),
        ("break", brk),
        ("captured", cap),
        ("composed", comp),
    ] {
        let k = v(&src);
        eprintln!("e {name} {k:?}");
        assert!(none_builtin(&k), "{name}: {k:?}");
    }
}

const MK: &str =
    "function mk(): any { const m: any = {}; m[process.argv[2]] = new K(); return m; }\n";

fn has_member(k: &crate::derive::KindFact) -> bool {
    k.top || k.funcs + k.builtin_objs > 0
}

/// ENUM2 (f) tsc's `assign` over PROGRAM-object sources (commandLineParser's
/// options copy), then ToPrimitive of the target: the target keeps its facts,
/// and the copy holds no built-in member of `Object.prototype` (none of them
/// is enumerable); ToPrimitive finds `toString` on the target's own chain.
#[test]
fn enum2_assign_copy_from_program_objects_adds_no_builtin_member() {
    let src = format!(
        "{R}{HAS}{ASSIGN}const out: any = assign({{}}, {{ k: new K() }});\nconsole.log([out].join(\",\"), `${{out}}`);\nconst r = new R(out.k);\nconsole.log(r.v.x);\n"
    );
    let k = v(&src);
    eprintln!("f k {k:?}");
    assert!(
        !k.top && k.objs.len() == 1 && k.objs[0].ends_with(":K"),
        "out.k {k:?}"
    );
    let src2 = format!("{R}{HAS}{ASSIGN}const out: any = assign(Object.create(null), {{ k: 1 }}, {{ j: 2 }});\nconst r = new R(out.zz);\nconsole.log(r.v);\n");
    let z = v(&src2);
    eprintln!("f zz {z:?}");
    assert!(none_builtin(&z), "out.zz {z:?}");
    // The source's keys unknown (commandLineParser's options map): on
    // 7f97c8a3fe the read walks into Object.prototype and copies its
    // members, `out` escapes through ToPrimitive (`out.k` = ⊤).
    let src3 = format!("{R}{HAS}{ASSIGN}{MK}const out: any = assign({{}}, mk());\nconsole.log([out].join(\",\"), `${{out}}`);\nconst r = new R(out.k);\nconsole.log(r.v.x);\n");
    let c = v(&src3);
    eprintln!("f computed k {c:?}");
    assert!(
        !c.top && c.objs.len() == 1 && c.objs[0].ends_with(":K") && c.builtin_objs == 0,
        "computed out.k {c:?}"
    );
    let src4 = format!("{R}{HAS}{ASSIGN}{MK}const out: any = assign(Object.create(null), mk());\nconst r = new R(out.zz);\nconsole.log(r.v);\n");
    let c = v(&src4);
    eprintln!("f computed zz {c:?}");
    assert!(
        !c.top && c.funcs == 0 && c.builtin_objs == 0,
        "computed out.zz {c:?}"
    );
}

/// ENUM2 (g) class instance, array and function sources.
#[test]
fn enum2_class_array_function_sources_add_no_builtin_member() {
    let pre = "class P { a: any = new K(); m() { return 1; } }\nfunction src() { return 0; }\n(src as any)[process.argv[2]] = new K();\nconst arr: any[] = [new K()];\narr[Number(process.argv[3])] = new K();\nconst pi: any = new P();\npi[process.argv[4]] = new K();\n";
    for (name, s, key) in [
        ("class", "pi", "a"),
        ("array", "arr", "0"),
        ("function", "src", "a"),
    ] {
        let kk = v(&format!("{R}{pre}{HAS}{ASSIGN}const out: any = assign(Object.create(null), {s});\nconst r = new R(out[\"{key}\"]);\nconsole.log(r.v);\n"));
        eprintln!("g {name} key {kk:?}");
        assert!(
            !kk.top && kk.objs.iter().any(|o| o.ends_with(":K")),
            "{name} key {kk:?}"
        );
        let z = v(&format!("{R}{pre}{HAS}{ASSIGN}const out: any = assign(Object.create(null), {s});\nconst r = new R(out.zz);\nconsole.log(r.v);\n"));
        eprintln!("g {name} zz {z:?}");
        // A class prototype is read by its fields: `m` and `constructor`
        // (the model does not split the declarations of a class, which are
        // not enumerable, from program writes); no `Object.prototype` member.
        let (fns, objs) = if name == "class" { (1, 1) } else { (0, 0) };
        assert!(
            !z.top && z.funcs <= fns && z.builtin_objs <= objs,
            "{name} zz {z:?}"
        );
    }
}

/// ENUM2 (h) a key the program wrote onto `Object.prototype` is enumerable:
/// the copy of a program object yields it.
#[test]
fn enum2_program_written_inherited_key_is_copied() {
    let src = format!("{R}{MK}function f() {{ return 1; }}\n(Object.prototype as any).mine = f;\nfunction copy(): any {{ const t: any = Object.create(null); const s: any = mk(); for (const p in s) {{ t[p] = s[p]; }} return t; }}\nconst r = new R(copy().mine);\nconsole.log(r.v);\n");
    let k = v(&src);
    eprintln!("h mine {k:?}");
    assert!(k.top || k.funcs >= 1, "mine {k:?}");
}

/// ENUM2 (i) a store through ⊤ under an unknown key may have written an
/// enumerable key onto `Object.prototype` (or onto `Object`): the copy
/// yields what that store wrote (ENUM1 open question 2).
#[test]
fn enum2_top_store_under_unknown_key_is_copied() {
    let pre = "function h() { return 3; }\nconst u: any = new Function(process.argv[2])();\nu[process.argv[3]] = h;\n";
    for (name, s) in [
        ("literal", "{ k: 1 }"),
        ("computed", "mk()"),
        ("Object", "Object"),
    ] {
        let src = format!("{R}{MK}{pre}function copy(): any {{ const t: any = Object.create(null); const s: any = {s}; for (const p in s) {{ t[p] = s[p]; }} return t; }}\nconst r = new R(copy().zz);\nconsole.log(r.v);\n");
        let k = v(&src);
        eprintln!("i {name} {k:?}");
        assert!(k.top || k.funcs >= 1, "{name} {k:?}");
    }
}

/// ENUM2 (j) controls on program objects: the full read stays; and links
/// left out of the rule read in full.
#[test]
fn enum2_program_object_controls_keep_the_full_lookup() {
    let other = format!("{R}function copy(a: any): any {{ const t: any = {{}}; const b: any = {{ x: 1 }}; for (const k in a) {{ t[k] = b[k]; }} return t; }}\nconst r = new R(copy(JSON.parse(process.argv[2])).zz);\nconsole.log(r.v);\n");
    let unk = format!(
        "{R}const o: any = {{ x: 1 }};\nconst r = new R(o[process.argv[2]]);\nconsole.log(r.v);\n"
    );
    let key_w = format!("{R}function copy(): any {{ const t: any = {{}}; const s: any = {{ x: 1 }}; for (let p in s) {{ p = p + process.argv[2]; t[p] = s[p]; }} return t; }}\nconst r = new R(copy().zz);\nconsole.log(r.v);\n");
    let obj_w = format!("{R}{MK}function copy(): any {{ const t: any = {{}}; let s: any = mk(); for (const p in s) {{ s = mk(); t[p] = s[p]; }} return t; }}\nconst r = new R(copy().zz);\nconsole.log(r.v);\n");
    // Left out: a host prototype given as a prototype (process's chain has
    // EventEmitter.prototype, whose methods are enumerable), a collection.
    let host = format!("{R}function copy(): any {{ const t: any = {{}}; const s: any = Object.create(Object.getPrototypeOf(process)); for (const p in s) {{ t[p] = s[p]; }} return t; }}\nconst r = new R(copy().zz);\nconsole.log(r.v);\n");
    let map = format!("{R}function copy(): any {{ const t: any = {{}}; const s: any = new Map(); for (const p in s) {{ t[p] = s[p]; }} return t; }}\nconst r = new R(copy().zz);\nconsole.log(r.v);\n");
    for (name, src) in [
        ("other", other),
        ("unk", unk),
        ("key_w", key_w),
        ("obj_w", obj_w),
        ("host", host),
        ("map", map),
    ] {
        let k = v(&src);
        eprintln!("j {name} {k:?}");
        assert!(has_member(&k), "{name}: {k:?}");
    }
}
