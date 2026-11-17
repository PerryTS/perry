//! STOR2: Web Storage (`localStorage` / `sessionStorage`) as perry's runtime
//! runs it (perry-runtime web_storage.rs): one Storage object per kind, born
//! on the first read of its global getter; `setItem` and a program [[Set]]
//! of a String key (WebIDL 3.9.2: the named property setter) store
//! ToString(value) under that key, mirrored as an own data property unless
//! the key is a member name of Storage.prototype or `length`; the object
//! never holds the program's value.
use super::*;

fn facts_of(src: &str) -> ProgramFacts {
    let path = "main.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "main", &path).expect("lowers");
    let list: Vec<(String, &perry_hir::Module)> = vec![(path, &hir)];
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    crate::compute(&list, &cfg)
}

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

/// The app's use (bloom/web storage.ts): setItem / getItem / length on
/// `localStorage`, next to a program object whose sites must stay proven.
const RED: &str = r#"
class P { v = 1; w = 2; }
const p = new P();
localStorage.setItem("k", String(process.argv[2]));
const v = localStorage.getItem("k") ?? "";
const n = localStorage.length;
sessionStorage.setItem("s", "1");
localStorage.removeItem("k");
const k0 = localStorage.key(0);
localStorage.clear();
console.log(v.length, n, k0, p);
const z = p.v + p.w;
console.log(z);
"#;

#[test]
fn st2_red_storage_methods_hold_nothing() {
    let pf = facts_of(RED);
    let d = pf.dump_text();
    for (k, mk) in [("v", "p.v + p.w"), ("w", "p.w;")] {
        assert!(proven(&verdict(&pf, RED, k, mk)), "{k} after {mk}:\n{d}");
    }
    for (k, mk) in [
        ("setItem", "localStorage.setItem(\"k\""),
        ("getItem", "localStorage.getItem(\"k\""),
        ("length", "localStorage.length;"),
        ("removeItem", "localStorage.removeItem"),
        ("key", "localStorage.key"),
        ("clear", "localStorage.clear"),
    ] {
        assert!(proven(&verdict(&pf, RED, k, mk)), "{k} after {mk}:\n{d}");
    }
}

/// A stored object comes back as a String: `getItem` never returns it, and
/// its `toString` (program code) runs.
const KEEP_STRING: &str = r#"
const o = { toString() { return "T" + process.argv[2]; }, q: 1 };
localStorage.setItem("k", o as any);
(localStorage as any).m = o;
const r = localStorage.getItem("k");
const r2 = (localStorage as any).m;
console.log(r, r2, o.q);
"#;

#[test]
fn st2_keep_stored_object_comes_back_as_a_string() {
    let pf = facts_of(KEEP_STRING);
    let d = pf.dump_text();
    assert!(proven(&verdict(&pf, KEEP_STRING, "q", "o.q")), "{d}");
    // `o` is coerced by setItem: its toString runs (a call of program code).
    assert!(
        pf.sites
            .iter()
            .any(|s| s.key == "toString" || d.contains("toString")),
        "{d}"
    );
}

/// `Storage.prototype.setItem` replaced by the program, chosen by argv: the
/// program function is the callee of the next call.
const KEEP_REPLACED: &str = r#"
const box = { hits: 0 };
const mine = function (this: any, k: string, v: string) { box.hits++; };
if (process.argv[2] === "r") (Storage.prototype as any).setItem = mine;
localStorage.setItem("a", "b");
console.log(box.hits);
"#;

#[test]
fn st2_keep_replaced_set_item_runs_program_code() {
    let pf = facts_of(KEEP_REPLACED);
    let d = pf.dump_text();
    // The program function may run: the read of `box.hits` after the call
    // must see its store (a Number), not a stale verdict.
    assert!(
        proven(&verdict(&pf, KEEP_REPLACED, "hits", "box.hits);")),
        "{d}"
    );
    // The call may run the program function: never the declared member.
    assert!(
        !proven(&verdict(
            &pf,
            KEEP_REPLACED,
            "setItem",
            "localStorage.setItem"
        )),
        "{d}"
    );
}

/// A key from argv that may be `getItem`: a store under it never replaces
/// the method (the runtime keeps it off the object); a read under it sees
/// the method or a stored String.
const KEEP_KEY: &str = r#"
const key = String(process.argv[2]);
(localStorage as any)[key] = 5;
const g = localStorage.getItem("x");
const h = (localStorage as any)[key];
for (const e in localStorage) console.log(e);
console.log(g, typeof h, localStorage.length);
"#;

#[test]
fn st2_keep_argv_key_and_for_in() {
    let pf = facts_of(KEEP_KEY);
    let d = pf.dump_text();
    assert!(
        proven(&verdict(
            &pf,
            KEEP_KEY,
            "getItem",
            "localStorage.getItem(\"x\")"
        )),
        "{d}"
    );
    assert!(
        proven(&verdict(&pf, KEEP_KEY, "length", "localStorage.length)")),
        "{d}"
    );
}
