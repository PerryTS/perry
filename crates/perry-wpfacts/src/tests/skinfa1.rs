//! SKINFA1 (F-A, the exact replacement of Rule R): `P[k]` in the body of
//! `for (k in P)`, `P = r.f1…fn` re-read in the body, is the read of the
//! enumerated object (ECMA-262 14.7.5.6 ForIn/OfHeadEvaluation, 14.7.5.9
//! EnumerateObjectProperties: a key is yielded only while it names an
//! enumerable property) iff nothing that runs in the body can change a link
//! of the path. Each program reads a parsed value (which may be an Array:
//! its non-enumerable methods) through the path into `dst`, then calls
//! `dst.foo` with `kept`: an Array.prototype method there hands `kept` to
//! unknown code.
use super::*;

fn whole_cfg(src: &str, cfg: &FactsConfig) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], cfg)
}

fn kept_escapes(pf: &ProgramFacts) -> bool {
    pf.templates
        .iter()
        .filter(|t| t.display.starts_with("t.ts"))
        .find(|t| t.fields.iter().any(|f| f.key == "keptmark"))
        .unwrap_or_else(|| panic!("no kept template\n{}", pf.dump_text()))
        .escape
        .is_some()
}

/// `pre` builds the values, `run` is the function with the loop.
fn prog(pre: &str, run: &str, call: &str) -> String {
    format!(
        "const unknown: any = (globalThis as any)[process.argv[2]];
         const kept = {{ keptmark: 1 }};
         const j: any = JSON.parse(process.argv[3]);
         const dst: any = {{}};
         {pre}
         {run}
         {call}
         const f: any = dst.foo;
         try {{ f.call(kept, unknown); }} catch (e) {{ console.log(\"t\"); }}"
    )
}

/// r1: nothing in the body can change `o`, `o.a` or `o.a.b`.
#[test]
fn skinfa1_r1_path_read_is_enumerated() {
    let src = prog(
        "const o: any = { a: { b: j.nodes[0] } };",
        "function run(o: any, dst: any) { for (const k in o.a.b) { dst[k] = o.a.b[k]; } }",
        "run(o, dst);",
    );
    let pf = whole_cfg(&src, &FactsConfig::default());
    assert!(!kept_escapes(&pf), "{}", pf.dump_text());
}

/// r2: the GLTFLoader shape (`addUnknownExtensionsToUserData`), `dst`
/// provably not on the path.
#[test]
fn skinfa1_r2_gltf_extensions() {
    let src = prog(
        "",
        "function add(def: any, dst: any) { for (const name in def.extensions) { dst[name] = def.extensions[name]; } }",
        "add(j.nodes[0], dst);",
    );
    let pf = whole_cfg(&src, &FactsConfig::default());
    assert!(!kept_escapes(&pf), "{}", pf.dump_text());
}

/// k1: the body assigns a path link (`o.a = other`).
#[test]
fn keep_skinfa1_k1_body_assigns_link() {
    let src = prog(
        "const o: any = { a: { b: j.nodes[0] } }; const other: any = { b: j.list };",
        "function run(o: any, dst: any, other: any) { for (const k in o.a.b) { o.a = other; dst[k] = o.a.b[k]; } }",
        "run(o, dst, other);",
    );
    assert!(kept_escapes(&whole_cfg(&src, &FactsConfig::default())));
}

/// k2: an unknown-key store into an object that may be `o` or `o.a`.
#[test]
fn keep_skinfa1_k2_unknown_key_store_may_hit_path() {
    let src = prog(
        "const o: any = { a: { b: j.nodes[0] } }; const other: any = { b: j.list };
         const x: any = process.argv.length > 9 ? o : o.a; const s: any = process.argv[5];",
        "function run(o: any, dst: any, x: any, s: any, other: any) { for (const k in o.a.b) { x[s] = other; dst[k] = o.a.b[k]; } }",
        "run(o, dst, x, s, other);",
    );
    assert!(kept_escapes(&whole_cfg(&src, &FactsConfig::default())));
}

/// k3: the body calls a function that writes a path link.
#[test]
fn keep_skinfa1_k3_call_writes_path() {
    let src = prog(
        "const o: any = { a: { b: j.nodes[0] } }; const other: any = { b: j.list };
         const g = () => { o.a = other; };",
        "function run(o: any, dst: any, g: any) { for (const k in o.a.b) { g(); dst[k] = o.a.b[k]; } }",
        "run(o, dst, g);",
    );
    assert!(kept_escapes(&whole_cfg(&src, &FactsConfig::default())));
}

/// k4: a path link is an accessor: two reads may give two objects.
#[test]
fn keep_skinfa1_k4_accessor_link() {
    let src = prog(
        "const o: any = {}; let n = 0;
         Object.defineProperty(o, 'a', { get() { n++; return n > 1 ? { b: j.list } : { b: j.nodes[0] }; } });",
        "function run(o: any, dst: any) { for (const k in o.a.b) { dst[k] = o.a.b[k]; } }",
        "run(o, dst);",
    );
    assert!(kept_escapes(&whole_cfg(&src, &FactsConfig::default())));
}

/// k5: the body deletes a path link: the inherited one shows through.
#[test]
fn keep_skinfa1_k5_delete_link() {
    let src = prog(
        "const o: any = Object.create({ a: { b: j.list } }); o.a = { b: j.nodes[0] };",
        "function run(o: any, dst: any) { for (const k in o.a.b) { delete o.a; dst[k] = o.a.b[k]; } }",
        "run(o, dst);",
    );
    assert!(kept_escapes(&whole_cfg(&src, &FactsConfig::default())));
}

/// k6: a path through an array element whose index the body changes.
#[test]
fn keep_skinfa1_k6_element_index_changes() {
    let src = prog(
        "const o: any = { list: [{ ext: j.nodes[0] }, { ext: j.list }] };",
        "function run(o: any, dst: any) { let i = 0; for (const k in o.list[i].ext) { i = 1; dst[k] = o.list[i].ext[k]; } }",
        "run(o, dst);",
    );
    assert!(kept_escapes(&whole_cfg(&src, &FactsConfig::default())));
}
