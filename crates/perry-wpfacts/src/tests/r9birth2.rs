// R9BIRTH2 (2026-11-15): fixtures for r9 sites left by R9BIRTH1, small JS.
// D: a read whose receiver set mixes a definite own data field with a key
//    absent from the object and its chain (three.js `o.isMesh` in a scene
//    traversal: Mesh holds `isMesh`, Scene / Group / Camera lack it). Each
//    receiver's [[Get]] (ECMA-262 10.1.8.1) gives its own data value or
//    `undefined`: the own-data-or-absent outcome, also when the slots
//    differ. Before rule D it read "slots differ across the receiver set".
// Proven = a verdict that is neither No(..) nor Throws.
use super::*;
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

/// (offset, is_call, is_store, verdict) of every site of `key` in a.ts.
fn sites(pf: &crate::ProgramFacts, key: &str) -> Vec<(u32, bool, bool, crate::Verdict)> {
    let mi = pf.module_paths.iter().position(|p| p == "a.ts").expect("a.ts") as u32;
    pf.sites
        .iter()
        .filter(|s| s.module == mi && s.key == key)
        .map(|s| (s.byte_offset, s.is_call, s.access.is_store(), s.verdict.clone()))
        .collect()
}

/// Proven by the construction-phase layout. R9STORE4: a known-object
/// store (rule A: the store adds or sets an own data property, exact for
/// any layout) is no layout claim, so these KEEP tests do not count it.
fn proven(v: &crate::Verdict) -> bool {
    !matches!(v, crate::Verdict::No(_) | crate::Verdict::Throws | crate::Verdict::KnownObjectStore)
}

fn show(tag: &str, pf: &crate::ProgramFacts, keys: &[&str]) {
    for k in keys {
        for s in sites(pf, k) {
            eprintln!("RB2 {tag} .{k} @{} call={} store={} verdict={:?}", s.0, s.1, s.2, s.3);
        }
    }
}

/// The read sites of `key` (not calls, not stores) at or after byte `from`.
fn reads_from(pf: &crate::ProgramFacts, key: &str, from: u32) -> Vec<crate::Verdict> {
    sites(pf, key)
        .into_iter()
        .filter(|s| !s.1 && !s.2 && s.0 >= from)
        .map(|s| s.3)
        .collect()
}

// The scene traversal: `pick` reads `isM` on a definite field (M) and on
// objects that never hold it (S, G).
const D_HEAD: &str = "class N { constructor() { this.name = \"\"; this.kids = []; } }
class M extends N { constructor() { super(); this.isM = true; this.count = 3; } }
class S extends N { constructor() { super(); this.bg = null; } }
class G extends N { constructor() { super(); } }
";

fn d_src(extra: &str) -> String {
    format!(
        "{D_HEAD}{extra}const all: any[] = [new S(), new G(), new M()];
const o = all[Number(process.argv[2] ?? \"0\") % 3];
// PICK
let n = 0;
if (o.isM) n += o.count;
console.log(n, all.length);
"
    )
}

fn pick_reads(pf: &crate::ProgramFacts, src: &str, key: &str) -> Vec<crate::Verdict> {
    let at = src.find("// PICK").expect("PICK") as u32;
    reads_from(pf, key, at)
}

// D1: a definite field beside receivers that lack the key: own data or
// `undefined` on every receiver.
#[test]
fn r9birth2_d1_definite_slot_beside_absent() {
    let src = d_src("");
    let pf = facts(&src);
    show("D1", &pf, &["isM"]);
    let r = pick_reads(&pf, &src, "isM");
    assert!(!r.is_empty() && r.iter().all(proven), "D1 o.isM not proven: {r:?}");
}

// D2: the key at different slots of two classes (and absent on a third).
#[test]
fn r9birth2_d2_definite_slots_differ() {
    let src = d_src("class K extends N { constructor() { super(); this.pad = 0; this.isM = false; this.count = 1; } }
all_k.push(new K());
")
    .replace("class G extends N { constructor() { super(); } }\n", "class G extends N { constructor() { super(); } }\nconst all_k: any[] = [];\n")
    .replace("[new S(), new G(), new M()]", "[new S(), new G(), new M(), all_k[0]]")
    .replace("% 3]", "% 4]");
    let pf = facts(&src);
    show("D2", &pf, &["isM"]);
    let r = pick_reads(&pf, &src, "isM");
    assert!(!r.is_empty() && r.iter().all(proven), "D2 o.isM not proven: {r:?}");
}

// D3 KEEP: M stores `isM` on one path only (a slot that may hold the hole)
// and M's prototype has a method `isM`: where the store did not run,
// [[Get]] finds the method. Not own data or `undefined`.
#[test]
fn r9birth2_d3_hole_slot_beside_prototype_member() {
    let src = d_src("")
        .replace(
            "class M extends N { constructor() { super(); this.isM = true; this.count = 3; } }",
            "class M extends N { constructor() { super(); if (process.argv.length > 3) this.isM = true; this.count = 3; }\n  isM() { return 1; } }",
        );
    let pf = facts(&src);
    show("D3", &pf, &["isM"]);
    let r = pick_reads(&pf, &src, "isM");
    assert!(!r.is_empty() && !r.iter().any(proven), "D3 o.isM proven, but [[Get]] may find M.prototype.isM: {r:?}");
}

/// Every store site of `key` (in a.ts) is proven / every one is not.
fn stores(pf: &crate::ProgramFacts, key: &str, want: bool) -> bool {
    let s: Vec<_> = sites(pf, key).into_iter().filter(|s| s.2).collect();
    !s.is_empty() && s.iter().all(|s| proven(&s.3) == want)
}

// L: a constructor that binds `const scope = this` (three.js
// LoadingManager): binding the alias observes nothing (rule L); a closure
// that captures it, or any read of it, is a use of `this`. `BODY` runs
// between the first two stores.
fn l_src(body: &str) -> String {
    format!(
        "function reg(x: any) {{ return x; }}
class LM {{ constructor(onLoad: any) {{
    const scope = this;
    let total = 0;
    this.onStart = undefined;
{body}    this.onLoad = onLoad;
    this.itemEnd = function (url: string) {{ total++; if (scope.onLoad !== undefined) scope.onLoad(url, total); }};
  }} }}
const lm = new LM((u: string, n: number) => console.log(u, n));
lm.itemEnd(String(process.argv[2] ?? \"a\"));
console.log(typeof lm.onStart);
"
    )
}

// L1: the stores before the first closure on the alias are birth stores.
#[test]
fn r9birth2_l1_this_alias_binding() {
    let pf = facts(&l_src(""));
    show("L1", &pf, &["onStart", "onLoad", "itemEnd"]);
    for k in ["onStart", "onLoad"] {
        assert!(stores(&pf, k, true), "L1 store .{k} not proven");
    }
}

// L2 KEEP: the alias is passed out before `this.onLoad` is stored: the
// callee may read the instance without `onLoad`.
#[test]
fn r9birth2_l2_alias_passed_out() {
    let pf = facts(&l_src("    reg(scope);\n"));
    show("L2", &pf, &["onLoad"]);
    assert!(stores(&pf, "onLoad", false), "L2 store .onLoad proven after reg(scope)");
}

// L3 KEEP: a closure on the alias runs before the store.
#[test]
fn r9birth2_l3_alias_closure_runs_first() {
    let pf = facts(&l_src("    const f = () => scope.onLoad;\n    reg(f());\n"));
    show("L3", &pf, &["onLoad"]);
    assert!(stores(&pf, "onLoad", false), "L3 store .onLoad proven after a closure on scope ran");
}

// L4 KEEP: a nested function declaration on the alias, called before the
// store (its declaration is hoisted: the call finds it bound).
#[test]
fn r9birth2_l4_alias_function_declaration_called_first() {
    let pf = facts(&l_src("    reg(g());\n    function g() { return scope.onLoad; }\n"));
    show("L4", &pf, &["onLoad"]);
    assert!(stores(&pf, "onLoad", false), "L4 store .onLoad proven after g() read scope");
}
