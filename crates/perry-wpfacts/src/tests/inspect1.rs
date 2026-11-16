//! INSPECT1 (2026-11-15): what the runtime formatter (`console.*`,
//! perry-runtime builtins/formatting.rs `format_jsvalue`) runs on a shown
//! value. It shows OWN properties only (an object: its own keys; a class:
//! `Object.keys`; a function: its own dynamic props) and calls only the
//! `[util.inspect.custom]` hook: an object's own symbol entry or the
//! `__perry_inspect_custom__` method of its class. A function or class
//! value is printed (`format_function_for_console`): no hook, no method of
//! it runs. Own getters run under `{ getters }` (console.dir,
//! util.inspect): kept.
use super::*;

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

fn calls(src: &str, key: &str) -> Vec<String> {
    let pf = one(src);
    let mut s: Vec<_> = pf.sites.iter().filter(|s| s.key == key && s.is_call).collect();
    s.sort_by_key(|s| s.byte_offset);
    let v: Vec<String> = s.iter().map(|s| format!("{:?}", s.verdict)).collect();
    eprintln!("INSPECT1 {key} {src:?}: {v:?}");
    assert!(!v.is_empty(), "no call site of {key}");
    v
}

fn mono(v: &[String]) -> bool {
    v.iter().all(|x| x == "T0Mono" || x.starts_with("T0Mono("))
}

const CLS: &str = "class E { on(t: any) { return t; } emit(t: any) { return this.on(t); } }\n\
class M extends E { m() { return 1; } }\n\
const a = new M(); const b = new M();\n\
declare const k: string;\n(E as any)[k] = Math.max;\n";

/// The class holds a builtin under an unknown key (a computed store, which
/// every named key sees: three.js EventDispatcher's allfields, n643).
/// RED: the instance is shown; its chain (`constructor`, the classes, the
/// methods) is not, and nothing of it is called.
#[test]
fn inspect1_shown_instance_class_not_escaped() {
    let v = calls(&format!("{CLS}console.log(\"x\", a);\nb.m();\n"), "m");
    assert!(mono(&v), "b.m() stays M::m: {v:?}");
}

/// RED: three.js shape: a log helper forwards rest params to console.error.
#[test]
fn inspect1_shown_through_rest_helper() {
    let src = format!(
        "{CLS}function error(...p: any[]) {{ const msg = \"T.\" + p.shift(); console.error(msg, ...p); }}\n\
         if ((a as any) === (b as any)) error(\"same\", a);\nb.m();\n"
    );
    let v = calls(&src, "m");
    assert!(mono(&v), "b.m() stays M::m: {v:?}");
}

/// RED: a class value shown: printed, no method of it runs.
#[test]
fn inspect1_shown_class_value() {
    let v = calls(&format!("{CLS}console.log(\"x\", M);\nb.m();\n"), "m");
    assert!(mono(&v), "b.m() stays M::m: {v:?}");
}

const HOLD: &str = "class C { m() { return 1; } }\nclass D { m() { return 2; } }\n\
const h: any = { f: new D() };\n";

/// KEEP: an own getter of a shown object may run (`{ getters }`): its
/// store is kept.
#[test]
fn inspect1_keep_own_getter_runs() {
    let src = format!(
        "{HOLD}const o = {{ get g() {{ h.f = new C(); return 1; }} }};\nconsole.log(o);\nh.f.m();\n"
    );
    let v = calls(&src, "m");
    assert!(!mono(&v), "getter may run: {v:?}");
}

/// KEEP: an own `[util.inspect.custom]` hook runs with `this` = the object.
#[test]
fn inspect1_keep_own_custom_hook_runs() {
    let src = format!(
        "{HOLD}const o: any = {{}};\no[Symbol.for(\"nodejs.util.inspect.custom\")] = function () {{ h.f = new C(); return \"o\"; }};\nconsole.log(o);\nh.f.m();\n"
    );
    let v = calls(&src, "m");
    assert!(!mono(&v), "hook may run: {v:?}");
}

/// KEEP: a value held in an own property is shown in turn (its own getter
/// may run).
#[test]
fn inspect1_keep_nested_own_getter_runs() {
    let src = format!(
        "{HOLD}const o = {{ in: {{ get g() {{ h.f = new C(); return 1; }} }} }};\nconsole.log(o);\nh.f.m();\n"
    );
    let v = calls(&src, "m");
    assert!(!mono(&v), "nested getter may run: {v:?}");
}

/// KEEP: a function-valued own data property is printed, not called.
#[test]
fn inspect1_function_property_not_called() {
    let src = format!("{HOLD}const o = {{ f() {{ h.f = new C(); }} }};\nconsole.log(o);\nh.f.m();\n");
    let v = calls(&src, "m");
    assert!(mono(&v), "o.f never runs: {v:?}");
}

/// INSPECT2 RED (three.js three.core.js `error()` l.2061 shape): a class
/// value reaches a LATER console argument through the rest helper while the
/// message may hold `%` (rule CONSOLE-PCT: the argument takes `Inspect`).
/// The runtime (util_format.rs 325-334, `util_format_s_inspects` 147: a
/// class ref is not GC_TYPE_OBJECT) prints `String(class)`: the class
/// source (to_string.rs `class_object_default_to_string`) unless the
/// program gave it a `toString`; none of its builtins runs user code.
#[test]
fn inspect2_class_later_console_arg_through_rest_helper() {
    let src = format!(
        "{CLS}function error(...p: any[]) {{ const msg = \"T.\" + p.shift(); console.error(msg, ...p); }}\n\
         declare const s: string;\nif ((a as any) === (b as any)) error(s, E);\nb.m();\n"
    );
    let v = calls(&src, "m");
    assert!(mono(&v), "b.m() stays M::m: {v:?}");
}

/// INSPECT2 RED: the same with the class passed directly after a `%s` message.
#[test]
fn inspect2_class_after_pct_message() {
    let v = calls(&format!("{CLS}declare const s: string;\nconsole.error(s, E);\nb.m();\n"), "m");
    assert!(mono(&v), "b.m() stays M::m: {v:?}");
}

/// INSPECT2 RED: `this` of a static method (the class) handed to the helper.
#[test]
fn inspect2_static_this_through_rest_helper() {
    let src = format!(
        "{CLS}function error(...p: any[]) {{ const msg = \"T.\" + p.shift(); console.error(msg, ...p); }}\n\
         declare const s: string;\nclass N extends E {{ static v() {{ error(s, this); }} }}\nN.v();\nerror(s, a);\nb.m();\n"
    );
    let v = calls(&src, "m");
    assert!(mono(&v), "b.m() stays M::m: {v:?}");
}
