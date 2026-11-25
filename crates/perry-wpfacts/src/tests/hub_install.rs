//! HUB2: a global installer loop (perry-web browser.ts:323, audio/index.ts:27)
//! `for (const [k, v] of Object.entries(classes)) g[k] = v` with `g` the
//! global object stores under exactly the own enumerable String keys of
//! `classes` (ECMA-262 20.1.2.5 Object.entries, 7.3.23
//! EnumerableOwnProperties; 14.7.5.6 binds one entry per iteration;
//! 10.1.9.1 OrdinarySet writes ToPropertyKey(k)). A `classes` with no key
//! `Object` leaves `globalThis.Object` the realms: the namespace pairs
//! generic call `globalThis.Object.getOwnPropertyNames(o)` reads the
//! original member.
use super::*;

fn ic(src: &str, key: &str) -> Vec<String> {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let mut set = perry_hir::PatchedBuiltins::new();
    perry_hir::scan_patched_builtins(&module, &mut set);
    set.namespace_pairs = true;
    perry_hir::set_patched_builtins(std::sync::Arc::new(set));
    let hir = perry_hir::lower_module(&module, "t", "t.ts");
    perry_hir::clear_patched_builtins();
    let hir = hir.expect("lowers");
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    cfg.namespace_pairs = true;
    let pf = compute(&[("t.ts".to_string(), &hir)], &cfg);
    let v: Vec<String> = pf
        .dump_text()
        .lines()
        .filter_map(|l| l.strip_prefix("I "))
        .filter_map(|l| {
            let (_, rest) = l.split_once(" .")?;
            let (k, verdict) = rest.split_once(char::is_whitespace)?;
            (k == key).then(|| verdict.to_string())
        })
        .collect();
    eprintln!("HUBI {key}: {v:?}");
    v
}

const PRE: &str = "class Fake { static getOwnPropertyNames(o: any): string[] { return [\"x\"]; } }
class Probe { n = 1; }
const g = globalThis as Record<string, unknown>;
";
const POST: &str = "const o = new Probe();
console.log(Object.getOwnPropertyNames(o).length);
";

fn src(classes: &str, install: &str) -> String {
    format!("{PRE}const classes: Record<string, unknown> = {classes};\n{install}\n{POST}")
}

fn exact(v: &[String]) -> bool {
    !v.is_empty() && v.iter().all(|x| x == "exact Object.getOwnPropertyNames")
}

const ENTRIES: &str = "for (const [k, v] of Object.entries(classes)) g[k] = v;";
const KEYS: &str = "for (const k of Object.keys(classes)) g[k] = classes[k];";

#[test]
fn hub_install_entries_keeps_object() {
    let v = ic(&src("{ Fake, Probe }", ENTRIES), "getOwnPropertyNames");
    assert!(exact(&v), "{v:?}");
}

#[test]
fn hub_install_entries_filtered_keeps_object() {
    let s = src(
        "{ Fake, Probe }",
        "for (const [k, v] of Object.entries(classes)) if (k === \"Fake\" || typeof g[k] === \"undefined\") g[k] = v;",
    );
    let v = ic(&s, "getOwnPropertyNames");
    assert!(exact(&v), "{v:?}");
}

#[test]
fn hub_install_keys_keeps_object() {
    let v = ic(&src("{ Fake, Probe }", KEYS), "getOwnPropertyNames");
    assert!(exact(&v), "{v:?}");
}

/// A `classes` that has the key `Object` replaces it: the generic call
/// may run `Fake.getOwnPropertyNames`.
#[test]
fn hub_install_entries_object_key_replaces() {
    let v = ic(
        &src("{ Object: Fake, Probe }", ENTRIES),
        "getOwnPropertyNames",
    );
    assert!(!exact(&v), "{v:?}");
}

#[test]
fn hub_install_keys_object_key_replaces() {
    let v = ic(&src("{ Object: Fake, Probe }", KEYS), "getOwnPropertyNames");
    assert!(!exact(&v), "{v:?}");
}

/// HUB2 (c09 browser.ts:222 `define(name, get)`): a literal-shape object
/// handed to unknown code shares its shape's class with the descriptor
/// literal of a computed-key define of the global object. The runtime
/// births a literal-shape object as a plain literal (perry-codegen
/// static_shape_ids.rs `BirthProto::Literal`): its [[Prototype]] is
/// %Object.prototype% (ECMA-262 13.2.5.5), which unknown code reached
/// through a handed value never writes (owner premise 2026-11-19). So the
/// descriptor's `set` and `value` stay absent (ToPropertyDescriptor
/// 6.2.6.5), the define installs only the named getters (`aa`, `bb`), and
/// `globalThis.Object` is untouched.
const DEFINE: &str = "declare function external(x?: unknown): any;
const ext: any = external();
class Probe { n = 1; }
function define(name: string, get: () => unknown): void { Object.defineProperty(globalThis, name, { get, configurable: true }); }
";

#[test]
fn hub_install_define_desc_shape_handed_away() {
    let s = format!(
        "{DEFINE}const get = (): number => 0;\next({{ get, configurable: true }});\ndefine(\"aa\", () => 1);\ndefine(\"bb\", () => 2);\n{POST}"
    );
    let v = ic(&s, "getOwnPropertyNames");
    assert!(exact(&v), "{v:?}");
}

#[test]
fn hub_install_define_alone_keeps_object() {
    let s = format!("{DEFINE}define(\"aa\", () => 1);\ndefine(\"bb\", () => 2);\n{POST}");
    let v = ic(&s, "getOwnPropertyNames");
    assert!(exact(&v), "{v:?}");
}
