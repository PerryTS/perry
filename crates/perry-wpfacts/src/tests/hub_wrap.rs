//! HUB: the chess-headless CHESS_GL_CPU wrapper loop (hooks.ts:101-112),
//! step by step from the per-key test shape (`own_keys_loop`) to the
//! c09 shape. Each evaluation of the arrow creates a new function object
//! (ECMA-262 15.3.4, 10.2.1) closing over that iteration's `const f`
//! (14.7.5.6 CreatePerIterationEnvironment): `gl.b("x")` runs `b` only.

use super::*;

const BASE: &str = "class Tex { id = 1; }
class Core { cs: any = null; core(s: string): void { this.cs = s; } }
class Mid extends Core { ms: any = null; mid(s: string): void { this.ms = s; } }
class GL extends Mid {
  at: any = 0;
  bs: any = null;
  a(x: number): void { this.at = x; }
  b(s: string): void { this.bs = s; }
  tex(): any { return new Tex(); }
}
class C2 { cx: any = null; c(s: string): void { this.cx = s; } }
";

const LOOP: &str = "for (let p = Object.getPrototypeOf(gl); p !== null && p !== Object.prototype; p = Object.getPrototypeOf(p)) {
  for (const k of Object.getOwnPropertyNames(p)) {
    if (k === \"constructor\" || seen.has(k)) continue;
    seen.add(k);
    const d = Object.getOwnPropertyDescriptor(p, k);
    if (d === undefined || typeof d.value !== \"function\") continue;
    const f = d.value as (...a: unknown[]) => unknown;
    gl[k] = (...a: unknown[]): unknown => {
      const t0 = performance.now();
      const r = f.apply(gl, a);
      inGl += performance.now() - t0; calls += 1;
      return r;
    };
  }
}
";

const USE: &str = "const g: any = gl0;
if (g !== null) { g.a(1); g.b(\"x\"); g.mid(\"y\"); g.core(\"z\"); const t = g.tex(); g.a(t); }
";

/// No Number reached `b`'s parameter, nor `mid`'s, nor `core`'s.
fn strings_only(pf: &ProgramFacts, what: &str) {
    for (c, f) in [("GL", "bs"), ("Mid", "ms"), ("Core", "cs")] {
        let k = field(tmpl(pf, c), f).kind.clone();
        assert!(
            k.prims & crate::model::prim::NUM == 0 && k.objs.is_empty() && !k.top,
            "{what}: {c}.{f} {k:?}"
        );
    }
}

fn run(make: &str, pre: &str, post: &str) -> ProgramFacts {
    facts(&format!(
        "{BASE}{make}const seen = new Set<string>();\nlet inGl = 0, calls = 0;\n{pre}{LOOP}{post}{USE}console.log(inGl, calls);\n"
    ))
}

const DIRECT: &str = "const gl0: any = new GL();\nconst gl: any = gl0;\n";

#[test]
fn hub_wrap_top_level() {
    strings_only(&run(DIRECT, "", ""), "top level");
}

#[test]
fn hub_wrap_in_a_frame_callback() {
    let pre = "const cbs: (() => void)[] = [];
function onAfterFrame(cb: () => void): void { cbs.push(cb); }
let wrapped = false;
onAfterFrame(() => { if (!wrapped) { wrapped = true;\n";
    let post = "return; } });\nfor (const cb of cbs) cb();\n";
    strings_only(&run(DIRECT, pre, post), "frame callback");
}

#[test]
fn hub_wrap_context_from_a_factory_map() {
    // getContext: the context comes out of a Map of factories (perry-web
    // elements.ts:447), one of several program classes.
    let make = "const contexts = new Map<string, () => unknown>();
contexts.set(\"webgl2\", () => new GL());
contexts.set(\"2d\", () => new C2());
class Canvas { getContext(k: string): unknown { const f = contexts.get(k); if (f === undefined) return null; return f() ?? null; } }
const cv = new Canvas();
const gl0 = cv.getContext(process.argv[2]) as unknown as Record<string, unknown> | null;
";
    let pre = "const cbs: (() => void)[] = [];
function onAfterFrame(cb: () => void): void { cbs.push(cb); }
let wrapped = false;
onAfterFrame(() => { if (!wrapped) { const gl = cv.getContext(\"webgl2\") as unknown as Record<string, unknown> | null; if (gl === null) return; wrapped = true;\n";
    let post = "return; } });\nfor (const cb of cbs) cb();\n";
    strings_only(&run(make, pre, post), "factory map");
}
