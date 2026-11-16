//! The native-extern assumption (owner 2026-11-16, "Assume it for now",
//! INTERIM; an exact link-time check of the library's imports replaces it):
//! a native-library extern whose manifest signature (package.json
//! `perry.nativeLibrary.functions`) has only primitive parameters and result
//! reaches no program object. Perry checks such an argument
//! (`js_native_abi_check_*`: a TypeError, never ToNumber / ToString), so no
//! program `valueOf` / `toString` runs; the call returns the descriptor's
//! kind. An extern the driver did not list (no manifest entry, or an object /
//! closure descriptor) stays unknown code as before.
use super::*;
use crate::NativeResult;

fn facts_of(src: &str, externs: &[(&str, NativeResult)]) -> ProgramFacts {
    let path = "main.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "main", &path).expect("lowers");
    let list: Vec<(String, &perry_hir::Module)> = vec![(path, &hir)];
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    cfg.native_externs = externs.iter().map(|(n, r)| (n.to_string(), *r)).collect();
    crate::compute(&list, &cfg)
}

/// The verdict of the first site `.key` after `marker`.
fn verdict(pf: &ProgramFacts, src: &str, key: &str, marker: &str) -> Verdict {
    let at = src.find(marker).expect("marker") as u32;
    pf.sites
        .iter()
        .filter(|s| s.key == key && s.byte_offset >= at)
        .min_by_key(|s| s.byte_offset)
        .unwrap_or_else(|| panic!("site .{key} after {marker:?} in\n{}", pf.dump_text()))
        .verdict
        .clone()
}

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

const LISTED: &[(&str, NativeResult)] = &[
    ("nx_add1", NativeResult::Number),
    ("nx_log", NativeResult::Undefined),
];

/// An f64 -> f64 and a string -> void extern in a loop next to an object
/// that a builtin call (`console.log`) sees afterwards.
const LOOP: &str = r#"
declare function nx_add1(x: number): number;
declare function nx_log(s: string): void;
class P { v = 1; w = 2; }
const p = new P();
const n = Number(process.argv[2] ?? "3");
let acc = 0;
for (let i = 0; i < n; i++) {
  acc += nx_add1(p.v);
  nx_log("x" + p.w);
}
console.log(acc, p);
const z = p.v + p.w;
console.log(z);
"#;

#[test]
fn w3e_red_all_primitive_externs_reach_no_object() {
    let pf = facts_of(LOOP, LISTED);
    let v = verdict(&pf, LOOP, "v", "const z");
    assert!(
        proven(&v),
        "p.v after the extern loop: {v:?}\n{}",
        pf.dump_text()
    );
    let w = verdict(&pf, LOOP, "w", "const z");
    assert!(proven(&w), "p.w after the extern loop: {w:?}");
    // The tag: both extern call sites rest on the assumption.
    assert_eq!(
        pf.native_extern_sites.len(),
        2,
        "{:?}",
        pf.native_extern_sites
    );
}

#[test]
fn w3e_keep_unlisted_extern_is_unknown_code() {
    // No manifest entry (or an object descriptor: the driver lists neither):
    // the call is unknown code as before, `p` handed to it escapes.
    let src = r#"
declare function nx_obj(o: object): number;
class P { v = 1; }
const p = new P();
nx_obj(p);
const z = p.v;
console.log(z);
"#;
    let pf = facts_of(src, LISTED);
    let v = verdict(&pf, src, "v", "const z");
    assert!(!proven(&v), "p.v after an unlisted extern took p: {v:?}");
    assert!(pf.native_extern_sites.is_empty());
}

#[test]
fn w3e_keep_empty_list_is_today() {
    // The knob off (`PERRY_FACTS_NATIVE_EXTERN=0`, an empty list): the loop
    // calls are unknown code again and `p` given to `console.log` escapes.
    let pf = facts_of(LOOP, &[]);
    assert!(pf.native_extern_sites.is_empty());
    let v = verdict(&pf, LOOP, "v", "const z");
    assert!(!proven(&v), "p.v with the assumption off: {v:?}");
}

#[test]
fn w3e_keep_object_argument_runs_no_value_of() {
    // An object with `valueOf` passed to an f64 slot (argv picks it): the
    // check throws a TypeError, `valueOf` never runs, the object reaches no
    // native: its own field stays proven, the store in `valueOf` never runs.
    let src = r#"
declare function nx_add1(x: number): number;
class H { k = 1; }
const h = new H();
const o = { valueOf() { (h as any).k = "s"; return 1; }, f: 2 };
try {
  if (process.argv[2] === "o") nx_add1(o as any);
} catch (e) {
  console.log(e instanceof TypeError);
}
const z = o.f + h.k;
console.log(z);
"#;
    let pf = facts_of(src, LISTED);
    let f = verdict(&pf, src, "f", "const z");
    assert!(proven(&f), "o.f: {f:?}\n{}", pf.dump_text());
    assert_eq!(pf.native_extern_sites.len(), 1);
}

#[test]
fn w3e_keep_module_binding_shadows_manifest_name() {
    // A module function of the manifest's name is program code, not the extern.
    let src = r#"
class P { v = 1; }
const keep: P[] = [];
function nx_add1(x: any): number { keep.push(x); return 1; }
const p = new P();
nx_add1(p);
const z = p.v;
console.log(z, keep.length);
"#;
    let pf = facts_of(src, LISTED);
    assert!(
        pf.native_extern_sites.is_empty(),
        "{:?}",
        pf.native_extern_sites
    );
}
