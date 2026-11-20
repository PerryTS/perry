//! SKINFA3: the three.js for-in path loops F-A rejects or opens (r7), one
//! reduced shape per cause. Each program reads a parsed value through the
//! path into `dst`, then calls `dst.foo` with `kept`: an Array.prototype or
//! Object.prototype member there hands `kept` to unknown code. RED: F-A is
//! too weak for the shape (the loop provably reads only enumerated
//! properties). `keep_`: the wide read is TRUE.
use super::*;

fn whole_cfg(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
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

/// (a) GLTFLoader.js:2341 `addUnknownExtensionsToUserData`: an object
/// literal in the body runs no program code (13.2.5.5).
#[test]
fn skinfa3_a_object_literal_in_body() {
    let src = prog(
        "const obj: any = { userData: {} };",
        "function add(known: any, object: any, def: any) { for (const name in def.extensions) { if (known[name] === undefined) { object.userData.ext = object.userData.ext || {}; object.userData.ext[name] = def.extensions[name]; dst[name] = def.extensions[name]; } } }",
        "add({}, obj, j.nodes[0]);",
    );
    assert!(!kept_escapes(&whole_cfg(&src)));
}

/// (b) GLTFLoader.js:1812: a program call in the body whose stores cannot
/// reach a path link (`setAttribute` stores into a fresh geometry's own
/// `attributes`, never into `src`).
#[test]
fn skinfa3_b_call_stores_off_path() {
    let src = prog(
        "class G { attributes: any = {}; setAttribute(n: string, a: any) { this.attributes[n] = a; return this; } }
         const s: any = { attributes: j.nodes[0] };",
        "function clone(src: any) { const g = new G(); for (const name in src.attributes) { g.setAttribute(name, src.attributes[name]); dst[name] = src.attributes[name]; } return g; }",
        "clone(s);",
    );
    assert!(!kept_escapes(&whole_cfg(&src)));
}

/// (e) GLTFLoader.js:1980: a TDZ check of a module `const` and a built-in
/// String method on the key run no program code.
#[test]
fn skinfa3_e_tdz_and_builtin_string_call() {
    let src = prog(
        "const s: any = { attributes: j.nodes[0] };",
        "function scan(p: any) { for (const name in p.attributes) { const n = NAMES[name] || name.toLowerCase(); dst[n] = p.attributes[name]; } }
         const NAMES: any = { POSITION: \"position\" };",
        "scan(s);",
    );
    assert!(!kept_escapes(&whole_cfg(&src)));
}

/// keep (GLTFLoader.js:1813 / 2002 in r7): the holder comes from unknown
/// code; a getter on it may replace the link between the head and the
/// read, so `src.m[k]` may read any member: the wide read is right.
#[test]
fn keep_skinfa3_holder_from_unknown_code() {
    let src = prog(
        "",
        "function copy(src: any) { for (const name in src.m) { dst[name] = src.m[name]; } }",
        "copy(unknown);",
    );
    assert!(kept_escapes(&whole_cfg(&src)));
}
