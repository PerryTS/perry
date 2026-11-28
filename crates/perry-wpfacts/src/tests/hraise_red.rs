//! HRAISE1 red fixture (c09 R1+R2 first TOP, hooks.ts:111:26 `f.apply(gl, a)`): a
//! computed-key `delete` whose receiver may be the global object is booked as a delete of
//! EVERY key there (gen_expr.rs IndexGet delete -> Cons::Hard E7Delete, key dropped). Every
//! global binding read then takes `undefined` (native_protocol.rs global_maybe_deleted) and the
//! global's any-key primitives, so the read of `Object` in rule T7's `p !== Object.prototype` admits
//! every value, %Object.prototype% stays in `p`, its own values under a computed key are the
//! opaque builtin function (intrinsic_proto_own -> undescribed_member), and `f.apply` hands
//! `gl`/`a` to it (HandRaise). ECMA-262 13.5.1.2: `delete o[k]` removes the one own property
//! ToPropertyKey(k) names; the realm's bindings (`Object`, ...) are non-enumerable (17), so no
//! for-in key of the global object names them. Expected RED on base (1337a157ab): R1, R3.
use crate::{compute, FactsConfig};

fn first_block(src: &str) -> String {
    let ast = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&ast, "t", "t.ts").expect("lowers");
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    crate::topstop::capture_on();
    let _ = compute(&[("t.ts".to_string(), &hir)], &cfg);
    let v = crate::topstop::take_captured();
    v.into_iter().next().unwrap_or_else(|| "NO STOP".to_string())
}

fn g_src(prelude: &str) -> String {
    let ctx = "class Core { n = 0; clear(m: number): void { this.n = m; } }\n\
class Base extends Core { drawArrays(a: number, b: number): number { return a + b; } }\n\
class GL1 extends Base { lineWidth(w: number): void { this.n = w; } }\n\
class GL2 extends Base { bindVertexArray(v: unknown): void { this.n = 1; } }\n\
class C2D { fillRect(x: number): void { console.log(x); } }\n\
const factories = new Map<string, () => unknown>();\n\
factories.set(\"webgl2\", () => new GL2()); factories.set(\"webgl\", () => new GL1()); factories.set(\"2d\", () => new C2D());\n\
class Canvas { c: unknown = null; getContext(k: string): unknown { if (this.c !== null) return this.c; const f = factories.get(k); if (f === undefined) return null; this.c = f(); return this.c; } }\n\
const canvas = new Canvas();\n\
const cbs: (() => void)[] = []; function onAfterFrame(f: () => void): void { cbs.push(f); }\n";
    let wrap = "let wrapped = false, calls = 0;\n\
onAfterFrame(() => {\n\
  if (!wrapped) {\n\
    const gl = canvas.getContext(\"webgl2\") as Record<string, unknown> | null;\n\
    if (gl === null) return;\n\
    wrapped = true;\n\
    const seen = new Set<string>();\n\
    for (let p = Object.getPrototypeOf(gl); p !== null && p !== Object.prototype; p = Object.getPrototypeOf(p)) {\n\
      for (const k of Object.getOwnPropertyNames(p)) {\n\
        if (k === \"constructor\" || seen.has(k)) continue;\n\
        seen.add(k);\n\
        const d = Object.getOwnPropertyDescriptor(p, k);\n\
        if (d === undefined || typeof d.value !== \"function\") continue;\n\
        const f = d.value as (...a: unknown[]) => unknown;\n\
        gl[k] = (...a: unknown[]): unknown => { const r = f.apply(gl, a); calls += 1; return r; };\n\
      }\n\
    }\n\
    return;\n\
  }\n\
});\n\
for (const f of cbs) f();\n";
    let use_ = "const g = canvas.getContext(\"webgl2\") as GL2 | null;\nif (g !== null) { g.clear(1); console.log(g.drawArrays(1, 2)); g.bindVertexArray(null); }\n";
    format!("{prelude}{ctx}{wrap}{use_}")
}


fn assert_no_stop(pre: &str) {
    let b = first_block(&g_src(pre));
    assert_eq!(b, "NO STOP", "{b}");
}

/// RED on base: `delete g[k]`, k one of two String constants, neither `Object`.
#[test]
fn hraise_const_key_delete_on_global_keeps_object_binding() {
    assert_no_stop("const g = globalThis as any;\nconst k = process.argv.length > 9 ? \"foo\" : \"bar\";\ndelete g[k];\n");
}

/// RED on base: three.js WebGLBindingStates shape, a map value that may be the global object
/// (`for (w in s) delete s[w]`): only enumerable own keys of the enumerated objects.
#[test]
fn hraise_forin_key_delete_on_global_keeps_object_binding() {
    assert_no_stop("const g = globalThis as any;\ng[\"window\"] = globalThis;\nconst maps: Record<string, any> = { a: { x: 1 }, b: g[\"window\"] };\nfor (const id in maps) { const s = maps[id]; for (const w in s) delete s[w]; }\n");
}

/// Control (green on base): a named delete of another global.
#[test]
fn hraise_named_delete_control() {
    assert_no_stop("const g = globalThis as any;\nif (g.foo !== undefined) delete g.foo;\n");
}

/// Control (green on base): no delete at all.
#[test]
fn hraise_no_delete_control() {
    assert_no_stop("");
}

// DEL-KEY (DELKEY1): the global bindings a computed-key delete may reach.
// What `x` (stored into `holder.xv`) may hold after `body`: `undefined` or ⊤.
fn xv_maybe_undef(body: &str) -> bool {
    let src = format!(
        "const k: any = process.argv[2];\nconst holder: any = {{ xv: 0 }};\n{body}\nconsole.log(holder.xv);\n"
    );
    let pf = super::facts(&src);
    let fs: Vec<_> = pf
        .templates
        .iter()
        .flat_map(|t| &t.fields)
        .filter(|f| f.key == "xv")
        .collect();
    assert!(!fs.is_empty(), "no field xv");
    fs.iter().any(|f| f.kind.top || f.kind.prims & 0x10 != 0)
}

/// DEL-KEY: a delete under a key that is one of finitely many String
/// constants removes those keys only; another global binding stays present.
#[test]
fn delkey_const_key_delete_spares_other_bindings() {
    let bad: Vec<&str> = [
        "const kk = k === \"a\" ? \"foo\" : \"bar\"; delete (globalThis as any)[kk]; const x: any = Int8Array; holder.xv = x;",
        "delete (globalThis as any)[\"foo\"]; const x: any = Int8Array; holder.xv = x;",
        "delete (globalThis as any).foo; const x: any = Int8Array; holder.xv = x;",
    ]
    .into_iter()
    .filter(|b| xv_maybe_undef(b))
    .collect();
    assert!(bad.is_empty(), "read as possibly absent: {bad:#?}");
}

/// Soundness controls: a delete whose key may name the binding keeps
/// `undefined`: the key's constants include it, the key is an unbounded
/// string, or it may be a Symbol / an object (ToPropertyKey, 7.1.19).
#[test]
fn delkey_reaching_or_unbounded_key_delete_keeps_undefined() {
    let bad: Vec<&str> = [
        "const kk = k === \"a\" ? \"Int8Array\" : \"bar\"; delete (globalThis as any)[kk]; const x: any = Int8Array; holder.xv = x;",
        "delete (globalThis as any)[k]; const x: any = Int8Array; holder.xv = x;",
        "const g: any = globalThis; delete g[String(k)]; const x: any = Int8Array; holder.xv = x;",
        "const kk: any = k === \"s\" ? Symbol.iterator : \"foo\"; delete (globalThis as any)[kk]; const x: any = Int8Array; holder.xv = x;",
        "const kk: any = k === \"o\" ? { toString() { return k; } } : \"foo\"; delete (globalThis as any)[kk]; const x: any = Int8Array; holder.xv = x;",
        "function drop(o: any, kk: string) { delete o[kk]; } drop(globalThis, String(k)); const x: any = Int8Array; holder.xv = x;",
    ]
    .into_iter()
    .filter(|b| !xv_maybe_undef(b))
    .collect();
    assert!(bad.is_empty(), "lost undefined: {bad:#?}");
}

// ENUM-DEL (DELKEY2): `for (w in s) delete s[w]` deletes only keys the for-in
// yields for the object deleted from; on the global object those are the keys
// the realm installs enumerable, the program's own globals and its numeric keys.
const LOOP: &str = "const maps: Record<string, any> = { a: { x: 1 }, b: globalThis };\n\
for (const id in maps) { const s = maps[id]; for (const w in s) delete s[w]; }\n";

/// ENUM-DEL: the realm's non-enumerable bindings stay present.
#[test]
fn enumdel_forin_delete_spares_realm_bindings() {
    let bad: Vec<String> = [
        "const x: any = Int8Array; holder.xv = x;",
        "const x: any = Object; holder.xv = x;",
        "(globalThis as any).mine = 1; const x: any = Math; holder.xv = x;",
        "const x: any = (globalThis as any).Object; holder.xv = x;",
    ]
    .into_iter()
    .map(|b| format!("{LOOP}{b}"))
    .filter(|b| xv_maybe_undef(b))
    .collect();
    assert!(bad.is_empty(), "read as possibly absent: {bad:#?}");
}

/// ENUM-DEL soundness: a key the for-in may yield is deleted: a realm global
/// installed enumerable, a program global, a realm name the program defined
/// enumerable, any key after an unknown-key write; a delete from another
/// object than the subject keeps the any-key delete.
#[test]
fn enumdel_forin_delete_keeps_undefined_for_enumerable_keys() {
    let bad: Vec<String> = [
        format!("{LOOP}const x: any = (globalThis as any).setTimeout; holder.xv = x;"),
        format!("{LOOP}const x: any = (globalThis as any).fetch; holder.xv = x;"),
        format!("(globalThis as any).mine = 1;\n{LOOP}const x: any = (globalThis as any).mine; holder.xv = x;"),
        format!("Object.defineProperty(globalThis, \"Int8Array\", {{ value: 1, enumerable: true, configurable: true, writable: true }});\n{LOOP}const x: any = (globalThis as any).Int8Array; holder.xv = x;"),
        format!("(globalThis as any)[k] = 1;\n{LOOP}const x: any = (globalThis as any).Int8Array; holder.xv = x;"),
        format!("(globalThis as any).Int8Array = 1;\n{LOOP}const x: any = (globalThis as any).Int8Array; holder.xv = x;"),
        // A delete made after the read was answered reaches it too.
        "(globalThis as any).Int8Array = 1; const x: any = (globalThis as any).Int8Array; holder.xv = x;\n\
function drop(): void { delete (globalThis as any).Int8Array; }\ndrop();".to_string(),
        "const maps: Record<string, any> = { a: { Int8Array: 1 }, b: globalThis };\n\
for (const id in maps) { const s = maps[id]; for (const w in s) delete (globalThis as any)[w]; }\n\
const x: any = (globalThis as any).Int8Array; holder.xv = x;".to_string(),
    ]
    .into_iter()
    .filter(|b| !xv_maybe_undef(b))
    .collect();
    assert!(bad.is_empty(), "lost undefined: {bad:#?}");
}


