//! Per-unit mode: a class value that leaves its declaration unit in any
//! form (its own binding of the same name, an alias binding, a re-assigned
//! binding, a namespace key) is modeled as other units use it, and an
//! aborted unit is unknown code holding everything its code names. The
//! per-unit template facts never claim more than whole-program facts.

use super::*;
use crate::TemplateFacts;

fn hirs(mods: &[(&str, &str)]) -> Vec<(String, perry_hir::Module)> {
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

fn run(mods: &[(&str, &str)], unit: bool, cfg: &FactsConfig) -> ProgramFacts {
    let h = hirs(mods);
    let list: Vec<(String, &perry_hir::Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    match unit {
        true => crate::compute_units(&list, cfg),
        false => crate::compute(&list, cfg),
    }
}

fn template<'p>(pf: &'p ProgramFacts, display: &str) -> &'p TemplateFacts {
    pf.templates
        .iter()
        .find(|t| t.display == display)
        .unwrap_or_else(|| panic!("no template {display}\n{}", pf.dump_text()))
}

fn field<'t>(t: &'t TemplateFacts, key: &str) -> &'t crate::derive::FieldFacts {
    t.fields
        .iter()
        .find(|f| f.key == key)
        .unwrap_or_else(|| panic!("no field {key} on {}", t.display))
}

/// The template facts codegen consumes that `u` claims and `w` does not
/// (per template display both have): a stable prefix, a fixed layout, per
/// field a slot, definiteness, readonly, a kind (`w`'s must be within
/// `u`'s), a stable typed array.
fn stronger(u: &ProgramFacts, w: &ProgramFacts) -> Vec<String> {
    let mut out = Vec::new();
    for tu in &u.templates {
        let Some(tw) = w.templates.iter().find(|t| t.display == tu.display) else {
            continue;
        };
        let d = &tu.display;
        if tu.prefix_stable && !tw.prefix_stable {
            out.push(format!("{d} prefix_stable"));
        }
        if tu.fixed_layout && !tw.fixed_layout {
            out.push(format!("{d} fixed_layout"));
        }
        for fu in &tu.fields {
            let Some(fw) = tw.fields.iter().find(|f| f.key == fu.key) else {
                continue;
            };
            let k = &fu.key;
            if fu.slot.is_some() && fw.slot.is_some() && fu.slot != fw.slot {
                out.push(format!("{d}.{k} slot"));
            }
            for (n, a, b) in [
                ("definite", fu.definite, fw.definite),
                ("readonly", fu.readonly, fw.readonly),
                ("ta_stable", fu.ta_stable, fw.ta_stable),
            ] {
                if a && !b {
                    out.push(format!("{d}.{k} {n}"));
                }
            }
            let (ku, kw) = (&fu.kind, &fw.kind);
            let within = ku.top
                || (!kw.top
                    && kw.prims & !ku.prims == 0
                    && kw.objs.iter().all(|o| ku.objs.contains(o))
                    && (kw.funcs == 0 || ku.funcs > 0)
                    && (kw.builtin_objs == 0 || ku.builtin_objs > 0));
            if !within {
                out.push(format!("{d}.{k} kind"));
            }
        }
    }
    out
}

fn both(mods: &[(&str, &str)]) -> (ProgramFacts, ProgramFacts) {
    let cfg = FactsConfig::default();
    (run(mods, true, &cfg), run(mods, false, &cfg))
}

fn assert_not_stronger(mods: &[(&str, &str)]) -> (ProgramFacts, ProgramFacts) {
    let (u, w) = both(mods);
    let s = stronger(&u, &w);
    assert!(
        s.is_empty(),
        "per-unit claims more than whole-program: {s:?}\n--- per-unit\n{}\n--- whole\n{}",
        u.dump_text(),
        w.dump_text()
    );
    (u, w)
}

const OWN_NAME: &str = r#"
var _C = class _C {
  constructor(a) { this.a = a; }
  get() { return this.a; }
  static make(a) { return new _C(a); }
};
var C = _C;
export { C };
"#;

#[test]
fn a_class_bound_to_a_var_of_its_own_name_is_constructed_by_its_importers() {
    // `var _C = class _C {..}` is the class `_C` and the binding `_C`
    // holding it, two declaration units. Other units construct it through
    // `C`: its defining unit models them (the contract), so its field holds
    // what they may pass and the read in `get` has receivers.
    let m = "import { C } from './a'; const c = new C('s'); console.log(c.get());";
    let (u, _) = assert_not_stronger(&[("a", OWN_NAME), ("m", m)]);
    let f = field(template(&u, "a.ts:_C"), "a");
    assert!(f.writers_ctor >= 1, "{f:?}\n{}", u.dump_text());
    assert!(
        f.kind.top || f.kind.prims & crate::model::prim::STR != 0,
        "{f:?}"
    );
    let dead: Vec<_> = u
        .sites
        .iter()
        .filter(|s| s.key == "a" && format!("{:?}", s.verdict).contains("dead site"))
        .collect();
    assert!(dead.is_empty(), "{dead:?}\n{}", u.dump_text());
}

#[test]
fn a_unit_naming_the_class_and_its_binding_imports_both() {
    // The unit of `_C.zero = new _C(..)` names the class `_C` (the `new`)
    // and the binding `_C` (the store): two imports of one name from two
    // units, each its own local. The store through the binding is a store
    // into the class object of another unit: reported, so the class is
    // broken where it is defined.
    let a = r#"
var _C = class _C {
  constructor(a) { this.a = a; }
  get() { return this.a; }
};
_C.zero = new _C(0);
var C = _C;
export { C };
"#;
    let m = "import { C } from './a'; const c = new C('s'); c.a = true; console.log(c.get());";
    let (u, w) = assert_not_stronger(&[("a", a), ("m", m)]);
    let fu = field(template(&u, "a.ts:_C"), "a");
    let fw = field(template(&w, "a.ts:_C"), "a");
    assert!(!fw.readonly && !fu.readonly, "{fu:?} {fw:?}");
}

#[test]
fn a_class_re_exported_under_an_alias_binding_is_its_class() {
    // `const V = Version; export { V as Version }`, and a binding
    // re-assigned to the class: both hold the class value.
    let a = r#"
class Version {
  constructor(major) { this.major = major; }
  compareTo(other) { return this.major - other.major; }
}
const V = Version;
export { V as Version };
export let W;
W = Version;
"#;
    let m = "import { Version, W } from './a'; const x = new Version(1); const y = new W('2'); console.log(x.compareTo(y));";
    assert_not_stronger(&[("a", a), ("m", m)]);
}

#[test]
fn an_export_names_the_binding_not_the_class_of_its_name() {
    // `export { _C }` exports the binding `_C`, re-assigned to `D`: the
    // importers construct `D`, never the class `_C`.
    let a = r#"
var _C = class _C { constructor(a) { this.a = a; } };
class D { constructor(b) { this.b = b; } }
_C = D;
export { _C };
"#;
    let m = "import { _C } from './a'; const x = new _C('s'); console.log(x.b);";
    let (u, _) = assert_not_stronger(&[("a", a), ("m", m)]);
    let f = field(template(&u, "a.ts:D"), "b");
    assert!(f.writers_ctor >= 1, "{f:?}\n{}", u.dump_text());
    // The importer's `x.b` reads a `D`: the binding holds it (the write) beside
    // the class `_C` (its initializer), whole-program and per unit alike.
    let s = u.sites.iter().find(|s| s.key == "b").expect("site x.b");
    assert!(
        s.top || s.receivers.iter().any(|r| r == "a.ts:D"),
        "{s:?}\n{}",
        u.dump_text()
    );
}

#[test]
fn a_unit_importing_a_class_and_a_binding_of_one_name_keeps_both() {
    // `f` names the class `_C` (`new _C`, the HIR's static class) and reads
    // the binding `_C`, now `D`: two imports of one name from two units,
    // each its own local. One local for both and `new _C` would construct
    // whatever the binding holds.
    let a = r#"
var _C = class _C { constructor(a) { this.a = a; } };
class D { constructor(b) { this.b = b; } }
_C = D;
function f(x) { const o = new _C(x); return o.a + (_C === D ? 1 : 0); }
export { f };
"#;
    let m = "import { f } from './a'; console.log(f(1));";
    let (u, _) = assert_not_stronger(&[("a", a), ("m", m)]);
    let s = u
        .sites
        .iter()
        .find(|s| s.key == "a" && s.module == 0)
        .expect("site o.a");
    assert_eq!(s.receivers, ["a.ts:_C"], "{s:?}\n{}", u.dump_text());
}

#[test]
fn a_class_reached_through_a_namespace_key_is_its_class() {
    // A namespace-merged object holding the class (`ns.Version = _Version`)
    // and the module namespace (`import * as p`): the class leaves through
    // a key.
    let a = r#"
var _Version = class _Version {
  constructor(major) { this.major = major; }
  compareTo(other) { return this.major - other.major; }
};
var ts = {};
ts.Version = _Version;
export { ts, _Version };
"#;
    let m = "import * as p from './a'; const x = new p.ts.Version(1); const y = new p._Version('2'); console.log(x.compareTo(y));";
    assert_not_stronger(&[("a", a), ("m", m)]);
}

#[test]
fn the_version_class_of_typescript_js() {
    // The shape of typescript.js's `Version` (u2201 the class, u2200 the
    // alias): per unit no stronger than whole-program.
    let a = r#"
var Debug;
((Debug2) => {
  function assert(e, msg) { if (!e) { throw new Error(msg); } }
  Debug2.assert = assert;
})(Debug || (Debug = {}));
var emptyArray = [];
var _Version = class _Version {
  constructor(major, minor = 0, prerelease = "") {
    if (typeof major === "string") { const p = major.split("."); major = +p[0]; minor = +p[1]; }
    Debug.assert(major >= 0, "major");
    this.major = major;
    this.minor = minor;
    this.prerelease = prerelease ? prerelease.split(".") : emptyArray;
  }
  compareTo(other) {
    if (this === other) return 0;
    if (other === void 0) return 1;
    return this.major - other.major || this.minor - other.minor;
  }
  increment() { return new _Version(this.major + 1, 0); }
  toString() { return `${this.major}.${this.minor}` + this.prerelease.join("."); }
};
_Version.zero = new _Version(0, 0, ["0"]);
var Version = _Version;
export { Version };
"#;
    let m = "import { Version } from './a'; const v = new Version('5.8'); console.log(v.compareTo(Version.zero), v.increment().toString());";
    let (u, _) = assert_not_stronger(&[("a", a), ("m", m)]);
    // The read of `other.major` has receivers (no dead site).
    let dead: Vec<_> = u
        .sites
        .iter()
        .filter(|s| format!("{:?}", s.verdict).contains("dead site"))
        .collect();
    assert!(dead.is_empty(), "{dead:?}\n{}", u.dump_text());
}

#[test]
fn a_class_expression_evaluated_in_a_run_once_body_is_its_class() {
    // typescript.js's shape: the whole file is a run-once arrow body, so
    // `var _Version = class _Version {..}` is the HIR's class
    // `_Version__class_expr_N` evaluated by `ClassExprFresh` at the
    // binding's statement. The evaluation is the class's definition (its
    // statics are the class's state): one unit with the class. Other units
    // construct it through the alias `Version`.
    let a = r#"
var ts = {}; ((module2) => {
var _Version = class _Version {
  constructor(major) { this.major = major; }
  compareTo(o) { return this.major - o.major; }
  inc() { return new _Version(this.major + 1); }
};
_Version.zero = new _Version(0);
var Version = _Version;
function f(s) { return new Version(s); }
function g(a, b) { return a.compareTo(b); }
ts.f = f; ts.g = g; ts.Version = Version;
console.log(g(f("1"), Version.zero));
})(typeof module !== "undefined" && module.exports ? module : { exports: ts });
"#;
    let (u, _) = assert_not_stronger(&[("a", a)]);
    let t = u
        .templates
        .iter()
        .find(|t| t.display.starts_with("a.ts:_Version__class_expr_"))
        .unwrap_or_else(|| panic!("{}", u.dump_text()));
    let f = field(t, "major");
    assert!(f.writers_ctor >= 1, "{f:?}\n{}", u.dump_text());
    let dead: Vec<_> = u
        .sites
        .iter()
        .filter(|s| s.key == "major" && format!("{:?}", s.verdict).contains("dead site"))
        .collect();
    assert!(dead.is_empty(), "{dead:?}\n{}", u.dump_text());
}

/// A unit whose walk stops at the node bound before the function that
/// uses another unit's class: `A` (walked first) is big, `B` (same unit:
/// they call each other) constructs `C` and stores into its field.
fn aborted_src() -> String {
    let mut s = String::from("import { C } from './a';\nfunction A(n) {\n  const xs = [];\n");
    for i in 0..400 {
        s.push_str(&format!("  xs.push({{ k{i}: n, v: [n, {i}] }});\n"));
    }
    s.push_str("  if (n > 1) { B(); }\n  return xs;\n}\n");
    s.push_str("function B() { const c = new C(1); c.a = 's'; A(0); return c; }\n");
    s.push_str("console.log(B());\n");
    s
}

#[test]
fn an_aborted_unit_holds_every_import_its_code_names() {
    let a = "export class C { constructor(a) { this.a = a; } get() { return this.a; } }";
    let m = aborted_src();
    let mods = [("a", a), ("m", m.as_str())];
    // The node bound: past the big unit's, under every other unit's.
    let free = run(&mods, true, &FactsConfig::default());
    let nodes: Vec<(String, u64)> = free
        .dump_text()
        .lines()
        .filter(|l| l.starts_with("# unit "))
        .map(|l| {
            let p = l.split_whitespace().nth(2).unwrap().to_string();
            let n = l
                .split("nodes=")
                .nth(1)
                .unwrap()
                .split_whitespace()
                .next()
                .unwrap();
            (p, n.parse().unwrap())
        })
        .collect();
    let big = nodes.iter().map(|(_, n)| *n).max().unwrap();
    let small = nodes
        .iter()
        .map(|(_, n)| *n)
        .filter(|&n| n < big)
        .max()
        .unwrap();
    assert!(big > 4 * small, "{nodes:?}");
    let cfg = FactsConfig {
        max_pts: Some(8 * (big / 2)),
        ..FactsConfig::default()
    };
    let u = run(&mods, true, &cfg);
    let dump = u.dump_text();
    // The guard held: exactly the big unit aborted.
    let aborted: Vec<&str> = dump
        .lines()
        .filter(|l| l.starts_with("# unit ") && l.contains("aborted=true"))
        .collect();
    assert_eq!(aborted.len(), 1, "{dump}");
    assert!(aborted[0].starts_with("# unit m.ts#"), "{dump}");
    // The aborted unit may do anything with `C`: broken, nothing claimed.
    let f = field(template(&u, "a.ts:C"), "a");
    assert!(!f.readonly, "{f:?}\n{dump}");
    assert!(!template(&u, "a.ts:C").fixed_layout, "{dump}");
}

#[test]
fn per_unit_facts_never_claim_more_than_whole_program() {
    // The diff on the probes of this file's cases, plus a plain export.
    let direct = r#"
export class Version {
  constructor(major) { this.major = major; }
  compareTo(other) { return this.major - other.major; }
}
export function isAtLeast(a, b) { return a.compareTo(b) >= 0; }
"#;
    let m = "import { Version, isAtLeast } from './a'; console.log(isAtLeast(new Version(1), new Version(2)));";
    assert_not_stronger(&[("a", direct), ("m", m)]);
    let m2 =
        "import { C } from './a'; const c = C.make(2); console.log(c.get(), new C([1]).get());";
    assert_not_stronger(&[("a", OWN_NAME), ("m", m2)]);
}

/// BAR's repro (`probe_version_test.rs`): typescript.js's `Version` class
/// as an ES module, constructed by another module through the alias.
const PROBE_A: &str = r#"var Debug;
((Debug2) => {
  function assert(e, msg) { if (!e) { throw new Error(msg); } }
  Debug2.assert = assert;
  function checkDefined(v, m) { assert(v !== undefined, m); return v; }
  Debug2.checkDefined = checkDefined;
})(Debug || (Debug = {}));
var emptyArray = [];
function isArray(v) { return Array.isArray(v); }
function every(a, f) { for (const x of a) { if (!f(x)) { return false; } } return true; }
var prereleasePartRegExp = /^(?:0|[1-9]\d*|[a-z-][a-z0-9-]*)$/i;
var buildPartRegExp = /^[a-z0-9-]+$/i;
function tryParseComponents(t) { const p = t.split("."); return { major: +p[0], minor: +p[1], patch: 0, prerelease: "", build: "" }; }
var _Version = class _Version {
  constructor(major, minor = 0, patch = 0, prerelease = "", build2 = "") {
    if (typeof major === "string") {
      const result = Debug.checkDefined(tryParseComponents(major), "Invalid version");
      ({ major, minor, patch, prerelease, build: build2 } = result);
    }
    Debug.assert(major >= 0, "Invalid argument: major");
    Debug.assert(minor >= 0, "Invalid argument: minor");
    Debug.assert(patch >= 0, "Invalid argument: patch");
    const prereleaseArray = prerelease ? isArray(prerelease) ? prerelease : prerelease.split(".") : emptyArray;
    const buildArray = build2 ? isArray(build2) ? build2 : build2.split(".") : emptyArray;
    Debug.assert(every(prereleaseArray, (s) => prereleasePartRegExp.test(s)), "Invalid argument: prerelease");
    Debug.assert(every(buildArray, (s) => buildPartRegExp.test(s)), "Invalid argument: build");
    this.major = major;
    this.minor = minor;
    this.patch = patch;
    this.prerelease = prereleaseArray;
    this.build = buildArray;
  }
  static tryParse(text) {
    const result = tryParseComponents(text);
    if (!result) return void 0;
    const { major, minor, patch, prerelease, build: build2 } = result;
    return new _Version(major, minor, patch, prerelease, build2);
  }
  compareTo(other) {
    if (this === other) return 0 /* EqualTo */;
    if (other === void 0) return 1 /* GreaterThan */;
    return compareValues(this.major, other.major) || compareValues(this.minor, other.minor) || compareValues(this.patch, other.patch) || comparePrereleaseIdentifiers(this.prerelease, other.prerelease);
  }
  increment(field) {
    switch (field) {
      case "major":
        return new _Version(this.major + 1, 0, 0);
      case "minor":
        return new _Version(this.major, this.minor + 1, 0);
      case "patch":
        return new _Version(this.major, this.minor, this.patch + 1);
      default:
        return Debug.assertNever(field);
    }
  }
  with(fields) {
    const {
      major = this.major,
      minor = this.minor,
      patch = this.patch,
      prerelease = this.prerelease,
      build: build2 = this.build
    } = fields;
    return new _Version(major, minor, patch, prerelease, build2);
  }
  toString() {
    let result = `${this.major}.${this.minor}.${this.patch}`;
    if (some(this.prerelease)) result += `-${this.prerelease.join(".")}`;
    if (some(this.build)) result += `+${this.build.join(".")}`;
    return result;
  }
};
var Version = _Version;
export const x = 1;
export { Version };
"#;

#[test]
fn the_bar_probe_version() {
    let m = r#"import { Version } from "./a"; console.log(new Version("1.2"));"#;
    let (u, w) = assert_not_stronger(&[("a", PROBE_A), ("m", m)]);
    for pf in [&u, &w] {
        let t = template(pf, "a.ts:_Version");
        assert!(field(t, "major").writers_ctor >= 1, "{t:?}");
    }
    // compareTo's `other.major` and toString's `this.prerelease.join` are
    // not dead per unit (whole-program: `other.major` is ⊤).
    let dead: Vec<_> = u
        .sites
        .iter()
        .filter(|s| s.module == 0 && format!("{:?}", s.verdict).contains("dead site"))
        .collect();
    assert!(dead.is_empty(), "{dead:?}\n{}", u.dump_text());
}

/// DECISIONS 193: `run`'s argument reaches methods of a literal (no closed
/// functions: their parameters hold argument markers, DECISIONS 44), each
/// storing a string under its own key, `n` calls deep.
fn method_chain(n: usize) -> String {
    let mut a = String::from("export function run(o) {\n  const h = {\n");
    for i in 0..n {
        let next = match i + 1 < n {
            true => format!(" h.f{}(x);", i + 1),
            false => String::new(),
        };
        a += &format!("    f{i}(x) {{ x.k{i} = \"s\";{next} }},\n");
    }
    a += "  };\n  h.f0(o);\n}\n";
    a
}

fn chain_reader(n: usize) -> String {
    let keys: Vec<String> = (0..n).map(|i| format!("k{i}: 0")).collect();
    let reads: Vec<String> = (0..n).map(|i| format!("o.k{i}")).collect();
    format!(
        "import {{ run }} from './a'; const o = {{ {} }}; run(o); console.log({});",
        keys.join(", "),
        reads.join(", ")
    )
}

/// The reader's literal: its field `k` holds a string (the store reached it).
fn reader_field_holds_str(u: &ProgramFacts, k: &str) -> bool {
    u.templates
        .iter()
        .filter(|t| t.display.starts_with("m.ts"))
        .flat_map(|t| t.fields.iter())
        .any(|f| f.key == k && f.kind.prims & crate::model::prim::STR != 0)
}

#[test]
fn a_store_two_calls_deep_reaches_the_outer_callers_argument() {
    // `f1` stores into what `f0` passes it, which is what `run`'s caller
    // passes: per unit `run`'s contract carries the store two calls deep
    // (the reader composes `f1`'s parameter into `f0`'s into `run`'s), so
    // `m`'s `o.k1` holds a string as in the whole program. Without the
    // composition `o.k1` would be a number only: stronger than whole
    // program.
    let (a, m) = (method_chain(2), chain_reader(2));
    let (u, _) = assert_not_stronger(&[("a", &a), ("m", &m)]);
    assert!(reader_field_holds_str(&u, "k1"), "{}", u.dump_text());
}

#[test]
fn a_store_through_a_recursion_reaches_the_outer_callers_argument() {
    // `f` and `g` call each other: their argument markers stand for each
    // other (a cycle in the member graph). `g`'s store reaches `run`'s
    // caller's object through `f`.
    let a = r#"
export function run(o, n) {
  const h = {
    f(x, k) { if (k > 0) { h.g(x, k - 1); } },
    g(y, k) { y.b = "s"; h.f(y, k); },
  };
  h.f(o, n);
}
"#;
    let m = "import { run } from './a'; const o = { b: 0 }; run(o, 3); console.log(o.b);";
    let (u, _) = assert_not_stronger(&[("a", a), ("m", m)]);
    assert!(reader_field_holds_str(&u, "b"), "{}", u.dump_text());
}

#[test]
fn a_store_costs_its_readers_not_the_call_path() {
    // DECISIONS 193: a store is handed to the readers its argument marker
    // stands for (here `run`'s parameter alone), not walked up every
    // argument marker above it (DECISIONS 118/121/187: tsc
    // typescript.js#u2245 stopped at the 200M work bound). A chain twice as
    // long costs about twice the work (64 -> 128: x1.98; the walk: x2.56).
    let w = |n: usize| {
        let (a, m) = (method_chain(n), chain_reader(n));
        let u = run(&[("a", &a), ("m", &m)], true, &FactsConfig::default());
        assert!(reader_field_holds_str(&u, &format!("k{}", n - 1)), "{}", u.dump_text());
        u.units
            .iter()
            .filter(|(p, _)| crate::decl_units::module_of(p) == "a.ts")
            .map(|(_, s)| s.work)
            .sum::<u64>()
    };
    let (w64, w128) = (w(64), w(128));
    assert!(w128 * 100 < w64 * 225, "work {w64} -> {w128}");
}

#[test]
fn a_closed_class_another_unit_extends_publishes_what_its_constructions_pass() {
    // DECISIONS 198 (scoreboard class_virtual): `Derived` (another unit)
    // extends closed `Base`, so Base's contract carries its constructor's
    // function contract and the stand-in constructor is that contract's
    // stand-in. `new Base(3)` passes 3 to it; the closed construction's
    // arguments are published from the first stub's parameters. Without
    // the wiring they were published empty: Base's `scale` read "no
    // value", stronger than the whole program's number.
    let a = "class Base { scale: number; constructor(k: number) { this.scale = k; } } class Derived extends Base { }\nfunction kernel(n: number): number { const cs: Base[] = [new Base(3), new Derived(5)]; return cs[n & 1].scale; }\nconsole.log(kernel(10));\n";
    let (u, _) = assert_not_stronger(&[("a", a)]);
    let f = field(template(&u, "a.ts:Base"), "scale");
    assert!(f.kind.prims & crate::model::prim::NUM != 0 || f.kind.top, "{f:?}\n{}", u.dump_text());
}

#[test]
fn a_layout_met_again_in_its_own_description_is_named_once() {
    // DECISIONS 199: L's field holds M, M's field holds L again (each a
    // copy one level deeper, as a forwarded copy read back every round
    // builds it). After the cut the inner L keeps its facts but describes
    // no field values, so the description stops growing.
    // A layout contract as `value_contract_of_toks` builds one, facts from a solve.
    let pf = run(&[("a", "const o = { a: 1 }; console.log(o.a);")], true, &FactsConfig::default());
    let facts = pf.templates.first().cloned().expect("a template");
    let mk = |d: &str| crate::units::ClassContract {
        display: d.to_string(),
        name: d.to_string(),
        class_id: 0,
        literal_shape: true,
        facts: facts.clone(),
        methods: Vec::new(),
        native_parent: None,
        parent: None,
        field_fns: Vec::new(),
        field_layouts: Vec::new(),
        field_values: Vec::new(),
        field_tas: Vec::new(),
        open_keys: false,
        statics: Vec::new(),
        statics_open: false,
        static_fns: Vec::new(),
        static_values: Vec::new(),
        closed: None,
        forwards: false,
        method_fns: Vec::new(),
        ctor_fn: None,
        birth_keys: Vec::new(),
        definite: Vec::new(),
        ctor_leaks: true,
        binding_prim: 0,
    };
    let with = |mut c: crate::units::ClassContract, inner: crate::units::ClassContract| {
        let vc = crate::units::ValueContract {
            layouts: vec![inner],
            ..Default::default()
        };
        c.field_values.push(("next".to_string(), vc));
        c
    };
    let deep = with(mk("L"), with(mk("M"), with(mk("L"), with(mk("M"), mk("L")))));
    let mut cut = deep.clone();
    crate::cut_described_again(&mut cut, &mut Vec::new());
    let m = &cut.field_values[0].1.layouts[0];
    assert_eq!(m.display, "M");
    let inner_l = &m.field_values[0].1.layouts[0];
    assert_eq!(inner_l.display, "L");
    assert!(inner_l.field_values.is_empty(), "{cut:?}");
    assert_ne!(format!("{cut:?}"), format!("{deep:?}"));
}

#[test]
fn a_function_passed_to_a_method_of_a_parameters_field_escapes() {
    // DECISIONS 202: `p.items` is a marker that is not the parameter's own,
    // so `p.items.map(cb)` is no call the caller's unit runs (DECISIONS
    // 144); its method is an unknown function, as a plain call through the
    // marker is. Before, only the method was marked: `cb` never ran, the
    // literal it makes was never born, and its facts were stronger than
    // the whole program's, where `render` runs `List` and `map` runs `cb`
    // (React `items.map(x => h(Item, { key: x }))`).
    let d = "export function render(f: any): any { return f({ items: [1] }); }\n";
    let m = r#"
        import { render } from "./d";
        const List = (p: any) => p.items.map((x: any) => (globalThis as any).zz({ key: x }));
        console.log(render(List));
        "#;
    let (u, _) = assert_not_stronger(&[("d", d), ("main", m)]);
    let t = u
        .templates
        .iter()
        .find(|t| t.display.starts_with("main.ts:__AnonShape") && t.birth_keys.iter().any(|k| k == "key"))
        .expect("the literal");
    assert!(t.escape.is_some(), "{t:?}");
}

#[test]
fn a_stand_in_reached_through_a_value_no_contract_describes_is_reported() {
    // DECISIONS 203: `b` calls an unknown export with the namespace as its
    // receiver, so the namespace and every export escape; `z` is broken and
    // `a` sinks it. Its getter returns `objectUtil`, another unit's stand-in
    // in `z`'s unit. `z`'s readers find an unknown value, so the stand-in is
    // not read through its owner's contract: it escapes in `z`'s unit, which
    // reports it, and its owner sinks it as the whole program does (zod's
    // `z.objectUtil` behind `z.array(...)`). Before, `z` escaped as a
    // publication and the stand-in left "with its owner's contract".
    let a = r#"
        const objectUtil: any = { mergeShapes: (x: any, y: any) => ({ ...x, ...y }) };
        const arrayType: any = (globalThis as any).unk;
        export const z = { get objectUtil() { return objectUtil; } };
        export { arrayType as array };
        "#;
    let b = "import * as zmod from \"./a\";\nconsole.log((zmod as any).array(1));\n";
    let (u, _) = assert_not_stronger(&[("a", a), ("b", b)]);
    let t = u
        .templates
        .iter()
        .find(|t| t.birth_keys.iter().any(|k| k == "mergeShapes"))
        .expect("objectUtil");
    assert!(t.escape.is_some(), "{t:?}");
}

#[test]
fn a_key_another_unit_stores_into_a_builtin_prototype_is_unknown_here() {
    // DECISIONS 204: `c` stores into `Function.prototype.call` (the builtin
    // prototypes are one object model-wide). In the whole program `check.call`
    // then reads that value: an unknown function, so `lit` escapes. Per unit
    // the store is in `c`'s unit; `a` read the builtin `call` and `lit` kept
    // facts the whole program refutes (fastify: an `Object.assign` onto
    // `Object.prototype` makes ajv's `checkOptions.call(this, removedOptions)`
    // an unknown call).
    let c = "(Function.prototype as any).call = (globalThis as any).zz;\nexport const c = 1;\n";
    let a = r#"
        function check(this: any, o: any) { return o.x; }
        export function mk() { const lit = { x: 1 }; check.call(null, lit); return 1; }
        "#;
    let m = "import { mk } from \"./a\";\nimport { c } from \"./c\";\nconsole.log(mk(), c);\n";
    let (u, _) = assert_not_stronger(&[("a", a), ("c", c), ("main", m)]);
    let t = u
        .templates
        .iter()
        .find(|t| t.display.starts_with("a.ts") && t.birth_keys.iter().any(|k| k == "x"))
        .expect("lit");
    assert!(t.escape.is_some(), "{t:?}");
}

#[test]
fn a_store_under_a_computed_key_into_a_parameter_reaches_the_callers_value() {
    // DECISIONS 205: `context[bodySchema] = schemas` stores under a computed
    // key (a symbol) into a parameter, whose node holds an argument marker
    // (DECISIONS 44). A named key is a flow to the marker's members (118); a
    // computed key went nowhere, so `schemas` kept its facts while the whole
    // program stores it into the caller's unknown value and it escapes
    // (fastify `validation.js`, `context[bodySchema] = contentTypeSchemas`).
    // Now it reaches the members under any key: the contract parameter,
    // whose store sinks it.
    let v = r#"
        const bodySchema = Symbol("body");
        export function compileSchemas(context: any, compile: any) {
          const schemas: any = {};
          schemas["a"] = compile(1);
          context[bodySchema] = schemas;
        }
        "#;
    let m = "import { compileSchemas } from \"./v\";\ncompileSchemas((globalThis as any).ctx, (x: any) => x);\n";
    let (u, _) = assert_not_stronger(&[("v", v), ("main", m)]);
    let t = u
        .templates
        .iter()
        .find(|t| t.display.starts_with("v.ts"))
        .expect("schemas");
    assert!(t.escape.is_some(), "{t:?}");
}
