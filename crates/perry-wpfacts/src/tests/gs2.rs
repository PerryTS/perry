//! GLOBS3 (2026-11-17): the own fields a computed read of the global object
//! takes under a key of known shape (`globalThis["on" + type]`). The global
//! object is ordinary (ECMA-262 9.3, 19): `O[P]` is ToPropertyKey, then
//! OrdinaryGet (13.3.2.1, 7.3.2, 10.1.8.1), so the read yields the own
//! property the key names and the chain only. The key's shape picks the
//! program globals it can spell (`key_fields`, `global_read_split`), not
//! every field of the global object; a fully unknown key keeps them all.
use super::*;

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

fn calls(src: &str, key: &str) -> Vec<String> {
    let pf = one(src);
    let mut s: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && s.is_call)
        .collect();
    s.sort_by_key(|s| s.byte_offset);
    let v: Vec<String> = s.iter().map(|s| format!("{:?}", s.verdict)).collect();
    eprintln!("GLOBS3 {key}: {v:?}");
    assert!(!v.is_empty(), "no call site of {key}");
    v
}

fn mono(v: &[String]) -> bool {
    v.iter().all(|x| x == "T0Mono" || x.starts_with("T0Mono("))
}

/// The global holds itself under `win`, a class under `A`, and a global
/// function `call` that replaces its second argument's `m`.
const HEAD: &str = "class C { m() { return 1; } }\n\
class A { x = 1; }\n\
const q: any = new C();\n\
const ty = String(process.argv[2] ?? \"x\");\n\
const g: any = globalThis;\n\
g.A = A;\n\
g.win = g;\n\
g.call = function (_t: any, e: any) { e.m = function () { return 2; }; };\n";

/// RED: `"on" + ty` spells neither `win` nor `A` nor `call`: the read is
/// not the global object, so `h.call` never runs the global `call`.
#[test]
fn gs2_shape_skips_fields_it_cannot_spell() {
    let src = format!(
        "{HEAD}const h = g[\"on\" + ty];\n\
         if (typeof h === \"function\") h.call(g, q);\nq.m();\n"
    );
    assert!(mono(&calls(&src, "m")), "q.m() stays C::m");
}

/// KEEP: a program global the shape spells holds the global object: the
/// read takes it, and `h.call` runs the global `call`.
#[test]
fn gs2_spelled_global_holding_the_global_is_kept() {
    let src = format!(
        "{HEAD}g.onwin = g;\n\
         const h = g[\"on\" + ty];\n\
         if (typeof h === \"function\") h.call(g, q);\nq.m();\n"
    );
    assert!(!mono(&calls(&src, "m")), "g.onwin.call is g.call");
}

/// KEEP: a fully unknown key reads every own field of the global object.
#[test]
fn gs2_unknown_key_keeps_every_field() {
    let src = format!(
        "{HEAD}const h = g[ty];\n\
         if (typeof h === \"function\") h.call(g, q);\nq.m();\n"
    );
    assert!(!mono(&calls(&src, "m")), "g[ty] may be the global object");
}

/// KEEP: a computed method call on the global under the shape calls the
/// spelled program global, which replaces `q.m`.
#[test]
fn gs2_spelled_method_call_runs() {
    let src = format!(
        "{HEAD}g.onfoo = function (e: any) {{ e.m = function () {{ return 2; }}; }};\n\
         g[\"on\" + ty](q);\nq.m();\n"
    );
    assert!(!mono(&calls(&src, "m")), "g.onfoo may replace q.m");
}

/// GLOBS4: the global holds a program global `obj` (a C); a store under
/// a String key the solver cannot name puts a D on the global object.
const H4: &str = "class C { m() { return 1; } }\n\
class D { m() { return 2; } }\n\
const ty = String(process.argv[2] ?? \"x\");\n\
const g: any = globalThis;\n\
g.obj = new C();\n";

/// The bodies the one `m` call may run (a read off the global object may
/// also be undefined, so the verdict is a set even for one body).
fn bodies_of_m(src: &str) -> Vec<String> {
    let pf = one(src);
    let s: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == "m" && s.is_call)
        .collect();
    assert_eq!(s.len(), 1, "one call site of m");
    let mut t = s[0].targets.clone();
    t.sort();
    eprintln!("GLOBS4 m: {:?} {t:?}", s[0].verdict);
    t
}

fn only(t: &[String], class: &str) -> bool {
    !t.is_empty() && t.iter().all(|x| x.contains(&format!("{class}::m")))
}

/// RED: a store under `"cls_" + ty` writes only a key that starts with
/// `cls_` (ECMA-262 10.1.9.1): the read of `obj` never sees its D.
#[test]
fn gs4_shaped_store_skips_names_it_cannot_spell() {
    let src = format!(
        "{H4}g[\"cls_\" + ty] = new D();\n\
         const o: any = (globalThis as any).obj;\no.m();\n"
    );
    assert!(only(&bodies_of_m(&src), "C"), "obj stays a C");
}

/// KEEP: a store under a fully unknown String key may write `obj`.
#[test]
fn gs4_unknown_key_store_reaches_every_name() {
    let src = format!(
        "{H4}g[ty] = new D();\n\
         const o: any = (globalThis as any).obj;\no.m();\n"
    );
    let t = bodies_of_m(&src);
    assert!(!only(&t, "C"), "g[ty] may be g.obj");
}

/// KEEP: a name the store's shape spells reads its value.
#[test]
fn gs4_shaped_store_reaches_a_spelled_name() {
    let src = format!(
        "{H4}g[\"cls_\" + ty] = new D();\n\
         const o: any = (globalThis as any).cls_x;\no.m();\n"
    );
    assert!(only(&bodies_of_m(&src), "D"), "cls_x is the D");
}

/// KEEP: a computed read whose shape meets the store's reads its value.
#[test]
fn gs4_shaped_read_meets_shaped_store() {
    let src = format!(
        "{H4}g[\"cls_\" + ty] = new D();\n\
         const o: any = g[\"cls_\" + ty];\no.m();\n"
    );
    assert!(only(&bodies_of_m(&src), "D"), "g[cls_ + ty] is the D");
}

// GLOBS7 (2026-11-17), R-JOBTHROW: a throw goes where the runtime sends it.
// An async body's throw rejects its own promise (27.7.5.1), a reaction
// handler's rejects the derived promise (27.2.2.1 NewPromiseReactionJob), a
// timer callback's goes to the host (HTML "report the exception"); none
// reaches an `await` of an unrelated promise (27.7.5.3 Await).
const GS7_R: &str = "class Scene { n = 0; traverse(cb: (o: Scene) => void): void { cb(this); } }\n\
function poke(k: string): void { if (k === \"never\") void fetch(\"http://h/\" + k).catch(() => null); }\n\
async function finale(): Promise<void> { poke(String(process.argv[2])); }\n\
setTimeout(() => { poke(String(process.argv[3])); }, 1);\n\
async function load(): Promise<void> {\n\
  const p = new Promise<{ scene: Scene }>((res) => res({ scene: new Scene() }));\n\
  const gltf = await p;\n\
  gltf.scene.traverse((o) => { o.n++; });\n\
}\n\
void finale().then(() => 1, () => 2);\n\
void load();\n";

#[test]
fn gs7_job_throw_never_reaches_an_unrelated_await() {
    assert!(mono(&calls(GS7_R, "traverse")));
}

/// The rejection still reaches its readers: an `await` in a try (the
/// async body's own promise), `.catch` of an executor's reject, and the
/// rejection handler of a promise whose reaction handler threw.
const GS7_K: &str = "class B { m(): number { return 1; } }\n\
async function bad(): Promise<number> { throw new B(); }\n\
async function f(): Promise<number> { try { await bad(); return 0; } catch (e) { return (e as B).m(); } }\n\
const p = new Promise<number>((_, rej) => rej(new B()));\n\
void p.catch((e: B) => e.m());\n\
void Promise.resolve(1).then(() => { throw new B(); }).then(() => 0, (e: B) => e.m());\n\
void f();\n";

#[test]
fn gs7_rejection_reaches_its_await_and_handlers() {
    let v = calls(GS7_K, "m");
    assert_eq!(v.len(), 3, "{v:?}");
    // Every catch and rejection reader also holds the runtime-error token
    // (THROWN, gen.rs): the one other receiver is the builtin Error.
    assert!(v.iter().all(|s| s.contains("builtin Error")), "{v:?}");
}

// GLOBS9 (2026-11-17), R-HOSTREPORT: what a timer or microtask callback or a
// module body throws is the host's to report (perry exception.rs
// `print_uncaught` reads `message`, `stack`, ToString); with no process
// listener nothing escapes.
const GS9_R: &str = "class B { m(): number { return 1; } }\n\
const b = new B();\n\
setTimeout(() => { try { b.m(); } catch (e) { throw e; } if (process.argv[2] === \"x\") throw b; }, 0);\n\
queueMicrotask(() => { if (process.argv[2] === \"y\") throw b; });\n\
if (process.argv[2] === \"z\") throw b;\n\
b.m();\n";

#[test]
fn gs9_host_report_does_not_escape() {
    let v = calls(GS9_R, "m");
    assert_eq!(v.len(), 2, "{v:?}");
    assert!(mono(&v), "{v:?}");
}

/// A process listener is handed the reported value: it escapes.
#[test]
fn gs9_process_listener_gets_the_report() {
    let src = format!("process.on(\"uncaughtException\", (e: any) => {{ e.m = 0; }});\n{GS9_R}");
    assert!(!mono(&calls(&src, "m")));
}

// GLOBS11 (2026-11-17): a settled resume (`AsyncStepResumeNow`) admits only
// a primitive or a fulfilled native promise, so the rejection of the promise
// awaited there never reaches the shared thrown cell, nor an unrelated await.
const GS11_R: &str = "const flag = process.argv.length > 9;\n\
async function bad(): Promise<void> { if (flag) fetch(\"http://127.0.0.1:1/\"); }\n\
async function waitBad(): Promise<string> { try { await bad(); return \"ok\"; } catch (e) { return \"caught\"; } }\n\
type Scene = { traverse(f: (n: number) => void): void };\n\
async function load(): Promise<{ scene: Scene }> { return { scene: { traverse(f) { f(7); } } }; }\n\
async function main(): Promise<void> { await waitBad(); const g = await load(); g.scene.traverse((n) => n); }\n\
void main();\n";

#[test]
fn gs11_settled_resume_never_rejects() {
    assert!(mono(&calls(GS11_R, "traverse")));
}
