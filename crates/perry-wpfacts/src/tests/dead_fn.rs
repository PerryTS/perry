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
