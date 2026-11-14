// DEADFN fixture (UNRUN: tool budget ended before the test build; place as
// crates/perry-wpfacts/src/tests/dead_fn.rs with `mod dead_fn;`, helpers as in
// tests/escaped_method_this.rs: hirs/run/read_site).
// Shape of r7: three.core.js `warn(...params)` -> `console.warn(message, ...params)`
// with message = 'THREE.' + <literal>; the console model applies Protocol::Inspect
// (keys include toJSON for %j) to every argument, so Material/Object3D::toJSON
// bodies are activated with `this` = the warned objects. No '%' can be in the
// format string, so the runtime never calls toJSON (Node util.formatWithOptions;
// perry-runtime formatting/util_format.rs): `this.w.z` inside toJSON is Dead.
use super::*;
use perry_hir::Module;

fn hirs(mods: &[(&str, &str)]) -> Vec<(String, Module)> {
    mods.iter()
        .map(|(name, src)| {
            let path = format!("{name}.ts");
            let module = perry_parser::parse_typescript(src, &path).expect("parses");
            let mut hir = perry_hir::lower_module(&module, name, &path).expect("lowers");
            for imp in &mut hir.imports {
                if let Some(n) = imp.source.strip_prefix("./") {
                    imp.resolved_path = Some(format!("{n}.ts"));
                }
            }
            (path, hir)
        })
        .collect()
}

fn run(h: &[(String, Module)], unit: bool) -> ProgramFacts {
    let list: Vec<(String, &Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    let cfg = FactsConfig {
        unit_io: true,
        ..Default::default()
    };
    match unit {
        true => crate::compute_units(&list, &cfg),
        false => crate::compute(&list, &cfg),
    }
}

#[allow(dead_code)]
/// The read `this.<key>` in module `a.ts`.
fn read_site<'a>(pf: &'a ProgramFacts, key: &str) -> &'a crate::SiteFacts {
    let mi = pf
        .module_paths
        .iter()
        .position(|p| p == "a.ts")
        .expect("a.ts") as u32;
    pf.sites
        .iter()
        .find(|s| s.module == mi && s.key == key && !s.is_call)
        .unwrap_or_else(|| panic!("a.ts .{key} in\n{}", pf.dump_text()))
}
const WARN: &str = "class A { constructor() { this.v = 1; } toJSON() { return this.w.z; } }
function warn(...p) { const m = 'X.' + p.shift(); console.warn(m, ...p); }
const a = new A();
warn('a: bad', a);
";
// Control 1: JSON.stringify really calls toJSON (25.5.2.2 SerializeJSONProperty).
const STRINGIFY: &str = "class A { constructor() { this.v = 1; } toJSON() { return this.w.z; } }
const a = new A();
console.log(JSON.stringify(a));
";
// Control 2: a format string that holds %j consumes the argument with toJSON.
const PCTJ: &str = "class A { constructor() { this.v = 1; } toJSON() { return this.w.z; } }
const a = new A();
console.warn('%j', a);
";
/// The verdict of `this.w.z`; `Dead` also when the solve never activated
/// the body (the site is not in the facts).
fn verdict_of(src: &str) -> crate::Verdict {
    let h = hirs(&[("a", src)]);
    let pf = run(&h, false);
    let mi = pf.module_paths.iter().position(|p| p == "a.ts").expect("a.ts") as u32;
    pf.sites
        .iter()
        .find(|s| s.module == mi && s.key == "z" && !s.is_call)
        .map_or(crate::Verdict::Dead, |s| s.verdict.clone())
}
#[test]
fn to_json_behind_a_percent_free_console_format_is_dead() {
    assert_eq!(verdict_of(WARN), crate::Verdict::Dead); // RED expected today: toJSON runs under Inspect
}
#[test]
fn to_json_under_stringify_runs() {
    assert_ne!(verdict_of(STRINGIFY), crate::Verdict::Dead);
}
#[test]
fn to_json_under_percent_j_runs() {
    assert_ne!(verdict_of(PCTJ), crate::Verdict::Dead);
}

// Rule CONSOLE-PCT controls (`builtins::console_args`).
const CLS: &str = "class A { constructor() { this.v = 1; } toJSON() { return this.w.z; } }
class S { constructor() { this.v = 1; } toString() { return this.w.z; } }
";
fn verdict_with(body: &str) -> crate::Verdict {
    verdict_of(&format!("{CLS}{body}"))
}
#[test]
fn percent_free_format_string_beside_a_spread_is_dead() {
    let body = "function warn(m, ...p) { console.warn('X.' + m, ...p); }
warn('a: bad', new A());
";
    assert_eq!(verdict_with(body), crate::Verdict::Dead);
}
#[test]
fn percent_s_calls_to_string() {
    // util_format.rs `%s`: an object with a user `toString` is String(v).
    assert_ne!(verdict_with("console.warn('a %s b', new S());\n"), crate::Verdict::Dead);
}
#[test]
fn unknown_format_string_keeps_inspect() {
    let body = "const f = process.env.FMT || 'x';
console.warn(f, new A());
";
    assert_ne!(verdict_with(body), crate::Verdict::Dead);
}
#[test]
fn object_first_argument_formats_nothing() {
    // util_format.rs: a first argument that is no String: every argument
    // is `format_jsvalue`d, no specifier runs.
    assert_eq!(verdict_with("console.warn({ k: 1 }, new A());\n"), crate::Verdict::Dead);
}
#[test]
fn leading_spread_with_percent_keeps_inspect() {
    let body = "const arr = [Math.random() > 2 ? '%j' : 'x', new A()];
console.warn(...arr);
";
    assert_ne!(verdict_with(body), crate::Verdict::Dead);
}
#[test]
fn leading_spread_without_percent_is_dead() {
    let body = "const arr = ['x', new A()];
console.warn(...arr);
";
    assert_eq!(verdict_with(body), crate::Verdict::Dead);
}
#[test]
fn other_console_methods_keep_inspect() {
    // `console.table` is not `js_util_format`: unchanged (Inspect).
    assert_ne!(verdict_with("console.table('x', new A());\n"), crate::Verdict::Dead);
}

// Rule OBJECT-TO-STRING (ECMA-262 7.1.1.1, 20.1.3.6): the string of an
// object whose ToPrimitive reaches only %Object.prototype.toString% is
// "[object Object]", which holds no `%`: the console format string beside
// `new A()` formats nothing, so A's toJSON never runs.
const OTS: &str = "class A { constructor() { this.v = 1; } toJSON() { return this.w.z; } }
class B { constructor() { this.v = 1; } }
";
fn ots(body: &str) -> crate::Verdict {
    verdict_of(&format!("{OTS}{body}"))
}
#[test]
fn object_literal_string_is_object_object() {
    assert_eq!(ots("console.warn('' + { k: 1 }, new A());\n"), crate::Verdict::Dead);
}
#[test]
fn class_instance_without_to_string_is_object_object() {
    assert_eq!(ots("console.warn('X.' + new B(), new A());\n"), crate::Verdict::Dead);
}
#[test]
fn to_string_tag_getter_runs() {
    // 20.1.3.6 step 15: Get(O, @@toStringTag) calls the getter.
    let src = "class T { constructor() { this.v = 1; } get [Symbol.toStringTag]() { return this.w.z; } }
console.log('X.' + new T());
";
    assert_ne!(verdict_of(src), crate::Verdict::Dead);
}
#[test]
fn to_string_tag_getter_result_is_any_string() {
    let body = "class T { get [Symbol.toStringTag]() { return Math.random() > 2 ? '%j' : 'T'; } }
console.warn('X.' + new T(), new A());
";
    assert_ne!(ots(body), crate::Verdict::Dead);
}
#[test]
fn program_to_string_runs() {
    let src = "class S { constructor() { this.v = 1; } toString() { return this.w.z; } }
console.log('X.' + new S());
";
    assert_ne!(verdict_of(src), crate::Verdict::Dead);
}
#[test]
fn program_to_string_result_is_its_string() {
    let body = "class S { toString() { return Math.random() > 2 ? '%j' : 's'; } }
console.warn('X.' + new S(), new A());
";
    assert_ne!(ots(body), crate::Verdict::Dead);
}
#[test]
fn program_value_of_result_is_its_string() {
    // `+` is ToPrimitive hint default: valueOf first (7.1.1.1 step 3).
    let body = "class V { valueOf() { return Math.random() > 2 ? '%j' : 'v'; } }
console.warn('X.' + new V(), new A());
";
    assert_ne!(ots(body), crate::Verdict::Dead);
}
#[test]
fn inherited_program_to_string_is_its_string() {
    let body = "class P { toString() { return Math.random() > 2 ? '%j' : 'p'; } }
class C extends P {}
console.warn('X.' + new C(), new A());
";
    assert_ne!(ots(body), crate::Verdict::Dead);
}
#[test]
fn escaped_instance_is_any_string() {
    let body = "const b = new B();
(globalThis as any).sink(b);
console.warn('X.' + b, new A());
";
    assert_ne!(ots(body), crate::Verdict::Dead);
}
#[test]
fn top_value_is_any_string() {
    assert_ne!(ots("console.warn('X.' + JSON.parse(process.env.J || '{}'), new A());\n"), crate::Verdict::Dead);
}
#[test]
fn later_written_to_string_is_its_string() {
    let body = "const b: any = new B();
console.warn('X.' + b, new A());
b.toString = () => (Math.random() > 2 ? '%j' : 'b');
";
    assert_ne!(ots(body), crate::Verdict::Dead);
}
// DEADFN5: three.core.js funnels every message through
// `function warn(...params) { const message = 'THREE.' + params.shift(); console.warn(message, ...params); }`.
// The format is `%`-possible only for a caller whose message has an any-string
// tail; such a caller passes no further argument, so the formatter consumes
// nothing there. A caller that passes an object has a literal message without `%`.
const FUNNEL: &str = "function warn(...params) { const message = 'P.' + params.shift(); console.warn(message, ...params); }
";
fn funnel(callers: &str) -> crate::Verdict {
    verdict_with(&format!("{FUNNEL}{callers}"))
}
#[test]
fn funnel_any_tail_alone_and_literal_with_object_is_dead() {
    assert_eq!(
        funnel("warn('a ' + process.env.H);\nwarn('literal no percent', new A());\n"),
        crate::Verdict::Dead
    );
}
#[test]
fn funnel_percent_j_with_object_runs() {
    assert_ne!(funnel("warn('a ' + process.env.H);\nwarn('x %j', new A());\n"), crate::Verdict::Dead);
}
#[test]
fn funnel_any_tail_with_object_runs() {
    assert_ne!(funnel("warn('literal no percent');\nwarn('a ' + process.env.H, new A());\n"), crate::Verdict::Dead);
}
#[test]
fn funnel_spread_of_unknown_length_runs() {
    let callers = "const arr = [];
if (Math.random() > 2) arr.push('a ' + process.env.H);
arr.push(new A());
warn(...arr);
warn('literal no percent');
";
    assert_ne!(funnel(callers), crate::Verdict::Dead);
}
#[test]
fn funnel_escaped_warn_runs() {
    let callers = "warn('a ' + process.env.H);
warn('literal no percent', new A());
globalThis.w = warn;
globalThis.a = new A();
eval(process.argv[2]);
";
    assert_ne!(funnel(callers), crate::Verdict::Dead);
}
// DEADFN6 (v): a run's rest array stored where later code writes into it:
// what the write stores is an element of that run's array (flow-insensitive),
// so the any-tail run's console call may format the object.
#[test]
fn funnel_stored_rest_written_later_runs() {
    let src = "let saved = [];
function warn(...params) { saved = params; const message = 'P.' + params.shift(); console.warn(message, ...params); }
warn('a ' + process.env.H);
warn('literal no percent');
saved.push(new A());
";
    assert_ne!(verdict_with(src), crate::Verdict::Dead);
}
// DEADFN6 (vi): `apply` spreads an array of unknown length into the rest.
#[test]
fn funnel_apply_runs() {
    let callers = "const arr = ['a ' + process.env.H, new A()];
warn.apply(null, arr);
warn('literal no percent');
";
    assert_ne!(funnel(callers), crate::Verdict::Dead);
}
// DEADFN6 (vi): `call` passes its arguments after `this` exactly
// (20.2.3.3), each `call` site its own run.
#[test]
fn funnel_call_per_site_is_dead() {
    let callers = "warn.call(null, 'a ' + process.env.H);
warn.call(null, 'literal no percent', new A());
";
    assert_eq!(funnel(callers), crate::Verdict::Dead);
}
// DEADFN8: three.core.js `warn` reassigns its rest array through
// `params = enhanceLogMessage(params)`, a function that may store into its
// parameter and returns it. Each run of `warn` gets back its own array.
const ENH: &str = "function enh(p) { if (process.env.T) { p[0] = 'x ' + p[0]; } return p; }
function warn(...params) { params = enh(params); const message = 'P.' + params.shift(); console.warn(message, ...params); }
";
const TWO_CALLERS: &str = "warn('a ' + process.env.H);\nwarn('literal no percent', new A());\n";
#[test]
fn funnel_through_identity_enhancer_is_dead() {
    assert_eq!(verdict_with(&format!("{ENH}{TWO_CALLERS}")), crate::Verdict::Dead);
}
// (i) The enhancer returns another call's array (a module variable).
#[test]
fn funnel_enhancer_returning_saved_array_runs() {
    let src = "let saved = [];
function enh(p) { const r = saved; saved = p; return r; }
function warn(...params) { params = enh(params); const message = 'P.' + params.shift(); console.warn(message, ...params); }
";
    assert_ne!(verdict_with(&format!("{src}{TWO_CALLERS}")), crate::Verdict::Dead);
}
// (ii) The enhancer stores a string that may hold `%` into the first element.
#[test]
fn funnel_enhancer_storing_unknown_string_runs() {
    let src = "function enh(p) { if (process.env.T) { p[0] = process.env.H; } return p; }
function warn(...params) { params = enh(params); const message = 'P.' + params.shift(); console.warn(message, ...params); }
";
    assert_ne!(verdict_with(&format!("{src}{TWO_CALLERS}")), crate::Verdict::Dead);
}
