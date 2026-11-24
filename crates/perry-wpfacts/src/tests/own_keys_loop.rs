//! A loop over the own keys of an object whose key set is known
//! (`for (const k of Object.keys(api))`) runs its body once per key, the
//! loop variable that one constant: `Object.keys(o)` is an ordinary Array
//! of `o`'s keys, its counted walk is outlined per key like a for-in, and
//! the iterator arm of the guarded for-of never runs for it. The shape is
//! perry-web chess-headless hooks.ts (every method wrapped on the instance).

use super::*;

const CLASSES: &str = "class Tex { id = 1; }
class C2 {
  lastY: any = 0;
  moveTo(x: number, y: number): void { this.lastY = y; }
  measure(s: string): number { return s.length; }
}
class GL {
  lastT: any = 0;
  lastTex: any = null;
  bindTexture(t: number, tex: any): void { this.lastT = t; this.lastTex = tex; }
  createTexture(): any { return new Tex(); }
}
class Out { v: any; constructor(v: any) { this.v = v; } }
function make(kind: string): any { return kind === \"2d\" ? new C2() : new GL(); }
const gl: any = make(process.argv[2]);
";

const WRAP: &str = "for (let p = Object.getPrototypeOf(gl); p !== null && p !== Object.prototype; p = Object.getPrototypeOf(p)) {
  for (const k of Object.getOwnPropertyNames(p)) {
    if (k === \"constructor\") continue;
    const d = Object.getOwnPropertyDescriptor(p, k);
    if (d === undefined || typeof d.value !== \"function\") continue;
    const f = d.value;
    gl[k] = (...a: any[]): any => f.apply(gl, a);
  }
}
";

const USE: &str = "gl.moveTo(1, 2);
const t = gl.createTexture();
gl.bindTexture(3, t);
const o = new Out(gl.measure(\"ab\"));
console.log(o.v);
";

fn kinds(pf: &ProgramFacts) -> [crate::KindFact; 4] {
    [
        field(tmpl(pf, "C2"), "lastY").kind.clone(),
        field(tmpl(pf, "GL"), "lastT").kind.clone(),
        field(tmpl(pf, "GL"), "lastTex").kind.clone(),
        field(tmpl(pf, "Out"), "v").kind.clone(),
    ]
}

fn wrapped(pre: &str) -> ProgramFacts {
    facts(&format!("{CLASSES}{pre}{WRAP}{USE}"))
}

#[test]
fn own_names_loop_with_unknown_names_runs_with_every_key() {
    // A prototype written under a key no name lists: its names are not
    // known, the body runs once with every key (the wrappers merge).
    let [_, _, _, v] = kinds(&wrapped(
        "(GL.prototype as any)[process.argv[3]] = function (): any { return new Tex(); };\n",
    ));
    assert!(v.objs.iter().any(|o| o.ends_with(":Tex")), "control {v:?}");
}

#[test]
fn own_keys_loop_over_a_literal_runs_per_key() {
    let pf = facts(
        "class Tex { id = 1; }
         class Out { v: any; constructor(v: any) { this.v = v; } }
         const api: any = { a(): any { return new Tex(); }, b(): any { return 1; } };
         const w: any = {};
         for (const k of Object.keys(api)) { const f = api[k]; w[k] = (...x: any[]): any => f.apply(api, x); }
         const t = w.a();
         const o = new Out(w.b());
         console.log(o.v, t);",
    );
    let v = field(tmpl(&pf, "Out"), "v").kind.clone();
    assert!(v.objs.is_empty() && !v.top, "w.b() {v:?}");
}

/// `w.b()` after `for (const k of Object.keys(api)) { <body> }`, written
/// inside `<pre> ... <post>`.
fn per_key(pre: &str, body: &str, post: &str) -> crate::KindFact {
    let pf = facts(&format!(
        "class Tex {{ id = 1; }}
         class Out {{ v: any; constructor(v: any) {{ this.v = v; }} }}
         const api: any = {{ a(): any {{ return new Tex(); }}, b(): any {{ return 1; }} }};
         const w: any = {{}};
         {pre} for (const k of Object.keys(api)) {{ {body} }} {post}
         const t = w.a();
         const o = new Out(w.b());
         console.log(o.v, t);"
    ));
    field(tmpl(&pf, "Out"), "v").kind.clone()
}

fn exact(v: crate::KindFact, what: &str) {
    assert!(v.objs.is_empty() && !v.top, "{what}: w.b() {v:?}");
}

const FP: &str = "function wrap(): void {";
const FQ: &str = "} wrap();";

#[test]
fn own_keys_loop_stores_each_method_under_its_key() {
    exact(per_key("", "w[k] = api[k];", ""), "top level");
    exact(per_key(FP, "w[k] = api[k];", FQ), "in a function");
}

#[test]
fn own_keys_loop_closure_calls_its_own_method() {
    exact(
        per_key(FP, "const f = api[k]; w[k] = (): any => f();", FQ),
        "closure",
    );
}

#[test]
fn own_keys_loop_rest_wrapper_applies_its_own_method() {
    let body = "const f = api[k]; w[k] = (...x: any[]): any => f.apply(api, x);";
    exact(per_key(FP, body, FQ), "function");
    exact(
        per_key("const run = (): void => {", body, "}; run();"),
        "arrow",
    );
}

#[test]
fn own_names_loop_runs_its_body_per_key() {
    // The run's key is the one property `getOwnPropertyDescriptor` describes:
    // `measure`'s wrapper calls `measure` only, so `gl.measure("ab")` is a
    // Number, never what `createTexture` returns.
    let [y, _, _, v] = kinds(&wrapped(""));
    assert!(v.objs.is_empty() && !v.top, "gl.measure() {v:?}");
    assert!(y.objs.is_empty() && !y.top, "moveTo's y {y:?}");
}
