// R9READ9 (2026-11-16): a Number-keyed computed store writes only canonical
// numeric names (ECMA-262 7.1.19 ToPropertyKey, 7.1.12.1 Number::toString).
use super::*;
use crate::model::{Model, TmplId};
use perry_hir::Module;

fn hirs(src: &str) -> Vec<(String, Module)> {
    let path = "a.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "a", &path).expect("lowers");
    vec![(path, hir)]
}

fn facts(src: &str) -> crate::ProgramFacts {
    let h = hirs(src);
    let list: Vec<(String, &Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    let cfg = FactsConfig {
        unit_io: true,
        ..Default::default()
    };
    crate::compute(&list, &cfg)
}

/// The template field row of `key` on class `class`: (writers_post, readonly).
fn row(pf: &crate::ProgramFacts, class: &str, key: &str) -> (usize, bool) {
    let t = pf
        .templates
        .iter()
        .find(|t| t.kind == "class" && t.display.contains(class))
        .unwrap_or_else(|| panic!("no template {class}\n{}", pf.dump_text()));
    let f = t
        .fields
        .iter()
        .find(|f| f.key == key)
        .unwrap_or_else(|| panic!("no field {key}\n{}", pf.dump_text()));
    eprintln!("RR9 {class}.{key} writers_post={} readonly={}", f.writers_post, f.readonly);
    (f.writers_post, f.readonly)
}

const CLASS: &str = "class V {
  x: number; name: string;
  constructor(x: number) { this.x = x; this.name = \"v\"; }
}
const o: any = new V(1);
const n = Number(process.argv[2] ?? \"0\");
";

fn src(store: &str) -> String {
    format!("{CLASS}{store}\nconsole.log(o.name, o.x);")
}

/// RED n1: `o[ n ] = v` with n a Number from argv: the store can name only a
/// canonical numeric string, never `name` (10.1.9.2 OrdinarySet writes P).
#[test]
fn r9read9_n1_number_key_store_skips_named_key() {
    let pf = facts(&src("function put(d: any, i: number) { d[i] = 7; }\nfor (let i = 0; i < n; ++i) put(o, i);"));
    assert_eq!(row(&pf, "V", "name"), (0, true));
}

/// KEEP n2: the key is `String( n )`: a String may be any name.
#[test]
fn r9read9_n2_string_key_kept() {
    let pf = facts(&src("o[String(n)] = 7;"));
    assert!(row(&pf, "V", "name").0 > 0);
}

/// KEEP n3: the key is a string from argv.
#[test]
fn r9read9_n3_argv_key_kept() {
    let pf = facts(&src("o[process.argv[3] ?? \"q\"] = 7;"));
    assert!(row(&pf, "V", "name").0 > 0);
}

/// KEEP n4: a number-or-string key.
#[test]
fn r9read9_n4_number_or_string_key_kept() {
    let pf = facts(&src("const k = n > 1 ? n : \"name\";\no[k] = 7;"));
    assert!(row(&pf, "V", "name").0 > 0);
}

/// KEEP n5: a Symbol key stays what it was (no named field written; the
/// rule does not apply: the key node is no Number).
#[test]
fn r9read9_n5_symbol_key_unchanged() {
    let pf = facts(&src("const s = Symbol(\"q\");\no[n > 1 ? s : \"name\"] = 7;"));
    assert!(row(&pf, "V", "name").0 > 0);
}

/// The solved model (the any-key writer table is not in ProgramFacts).
fn model(src: &str) -> Model {
    let h = hirs(src);
    let mut g = crate::gen::Gen::new(vec![("a.ts", &h[0].1)], false, false);
    g.m.max_work = 50_000_000;
    g.register_all();
    g.link_exports();
    g.walk_all();
    g.feed_parent_values();
    g.m.summarize_and_replay();
    g.m.solve();
    g.m
}

/// Post-birth any-key writers on templates selected by `pick`, with
/// whether each may write `key`.
fn any_post(m: &Model, pick: impl Fn(&Model, TmplId) -> bool, key: &str) -> Vec<bool> {
    let a = *m.atom_index.get(key).expect("atom");
    let mut v = Vec::new();
    for (&tm, w) in &m.any_writers {
        if pick(m, tm) {
            for &(s, k) in &w.post {
                v.push(m.any_store_may_name(tm, s, k, a));
            }
        }
    }
    eprintln!("RR9 any_post {key}: {v:?}");
    v
}

/// RED n1 (model): the Number-keyed store is an any-key writer of V, and it
/// writes no `name`.
#[test]
fn r9read9_n1m_number_key_writer_not_named() {
    let m = model(&src("function put(d: any, i: number) { d[i] = 7; }\nfor (let i = 0; i < n; ++i) put(o, i);"));
    let v = any_post(&m, |m, tm| !m.tmpl_may_be_array(tm), "name");
    assert!(!v.is_empty() && v.iter().all(|&x| !x), "{v:?}");
}

/// KEEP n6: an Array subclass: an index store sets `length` (10.4.2.1).
#[test]
fn r9read9_n6_array_length_kept() {
    let m = model(
        "class A extends Array { tag = 0; }
const a: any = new A();
const n = Number(process.argv[2] ?? \"0\");
a[n] = 1;
console.log(a.length, a.tag);",
    );
    let v = any_post(&m, |m, tm| m.tmpl_may_be_array(tm), "length");
    assert!(v.iter().all(|&x| x), "{v:?}");
}

/// KEEP n7: an accessor named "0" on the class prototype: the store runs
/// the setter (user code); the setter's own store of `name` stays a named
/// writer.
#[test]
fn r9read9_n7_index_setter_kept() {
    let pf = facts(&format!(
        "class W {{ x = 1; name = \"w\"; set 0(v: any) {{ this.name = String(v); }} }}
const o: any = new W();
const n = Number(process.argv[2] ?? \"0\");
o[n] = 7;
console.log(o.name);"
    ));
    assert!(row(&pf, "W", "name").0 > 0);
}

fn reads(pf: &crate::ProgramFacts, key: &str) -> Vec<String> {
    let mi = pf.module_paths.iter().position(|p| p == "a.ts").expect("a.ts") as u32;
    let v: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.module == mi && s.key == key && !s.access.is_store())
        .map(|s| format!("{:?} {:?}", s.verdict, s.receivers))
        .collect();
    for verdict in &v {
        eprintln!("RR9 .{key} verdict={verdict}");
    }
    v
}

const FNS: &str = "const n = Number(process.argv[2] ?? \"3\");
const x: any = n > 2 ? [1, 2] : n > 1 ? Number : n > 0 ? Math.max : Number.parseFloat;
";

/// m1: `.map` of a builtin constructor (Number) or a builtin method value
/// (Math.max, Array.prototype.map): absent on the function and its chain
/// (Function.prototype, Object.prototype): `undefined`; the Array arm
/// reads its own builtin member.
#[test]
fn r9read9_m1_builtin_function_map_absent() {
    let pf = facts(&format!("{FNS}console.log(typeof x.map);"));
    let v = reads(&pf, "map");
    assert!(!v.is_empty() && v.iter().all(|x| !x.starts_with("No(")), "{v:?}");
}

/// KEEP m2: the program stores `map` on the builtin method value.
#[test]
fn r9read9_m2_store_on_function_kept() {
    let pf = facts(&format!("{FNS}(Math.max as any).map = 1;\nconsole.log(typeof x.map);"));
    let v = reads(&pf, "map");
    assert!(v.iter().any(|x| x.starts_with("No(")), "{v:?}");
}

/// KEEP m3: the program writes `map` onto Function.prototype.
#[test]
fn r9read9_m3_function_prototype_write_kept() {
    let pf = facts(&format!("{FNS}(Function.prototype as any).map = 1;\nconsole.log(typeof x.map);"));
    let v = reads(&pf, "map");
    assert!(v.iter().any(|x| x.starts_with("No(")), "{v:?}");
}

/// KEEP m4: `length` is an own property of a builtin function (not absent:
/// since R9READ10 the present own member, a PerKind arm, never
/// OwnDataOrAbsent).
#[test]
fn r9read9_m4_own_length_not_absent() {
    let pf = facts("const n = Number(process.argv[2] ?? \"3\");
const x: any = n > 1 ? Math.max : Math.min;
console.log(x.length);");
    let v = reads(&pf, "length");
    assert!(!v.is_empty() && v.iter().all(|x| !x.starts_with("OwnDataOrAbsent")), "{v:?}");
}
