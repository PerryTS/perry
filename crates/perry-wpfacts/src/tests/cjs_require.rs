//! A literal `require` of a compiled CommonJS module is a static edge (design
//! CJS_LINK 2026-10-26 §3): a read of the target's record binding, then its
//! `exports` slot. A computed specifier keeps the registry summary.

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
    match unit {
        true => crate::compute_units(&list, &FactsConfig::default()),
        false => crate::compute(&list, &FactsConfig::default()),
    }
}

// The target's side of the wrapper: its record binding, assigned by the
// factory before the body.
fn target(class: &str, value: &str) -> String {
    format!(
        "class {class} {{ x = {value}; }}\nexport var __perry_cjs_record;\n(function () {{ __perry_cjs_record = {{ exports: new {class}(), loaded: false }}; }})();\n"
    )
}

const REQUIRER: &str = r#"
    import { __perry_cjs_record as __perry_cjs_rec_0 } from "./left";
    function __perry_require_static_0() {
        const record = __perry_cjs_init(__perry_cjs_rec_0);
        return record.exports;
    }
    const left = __perry_require_static_0();
    console.log(left.x);
"#;

fn x_sites(pf: &ProgramFacts, module: &str) -> Vec<crate::SiteFacts> {
    let mi = pf
        .module_paths
        .iter()
        .position(|m| m == module)
        .unwrap_or_else(|| panic!("{module} in {:?}", pf.module_paths)) as u32;
    pf.sites
        .iter()
        .filter(|s| s.key == "x" && s.module == mi)
        .cloned()
        .collect()
}

#[test]
fn a_static_edge_takes_its_target_record() {
    let h = hirs(&[
        ("left", &target("Left", "1")),
        ("right", &target("Right", "\"right\"")),
        ("main", REQUIRER),
    ]);
    let pf = run(&h, false);
    let sites = x_sites(&pf, "main.ts");
    assert_eq!(sites.len(), 1, "{}", pf.dump_text());
    assert_eq!(sites[0].verdict, Verdict::T0Mono, "{}", pf.dump_text());
    assert!(
        sites[0].receivers.iter().all(|r| r.ends_with(":Left")),
        "{:?}\n{}",
        sites[0].receivers,
        pf.dump_text()
    );
    // Per unit is never stronger than whole program.
    let pu = run(&h, true);
    for s in x_sites(&pu, "main.ts") {
        if s.verdict == Verdict::T0Mono {
            assert!(
                s.receivers.iter().all(|r| r.ends_with(":Left")),
                "{}",
                pu.dump_text()
            );
        }
    }
}

#[test]
fn a_computed_require_keeps_the_registry() {
    // A specifier the wrapper could not resolve sees every published record.
    let left = format!(
        "{}__perry_register_path_module(\"/cjsq-left.js\", __perry_cjs_record);\n",
        target("Left", "1")
    );
    let right = format!(
        "{}__perry_register_path_module(\"/cjsq-right.js\", __perry_cjs_record);\n",
        target("Right", "\"right\"")
    );
    let main = r#"
        function pick(n) { return n ? "/cjsq-left.js" : "/cjsq-right.js"; }
        const any = __perry_require_path_module(pick(Math.random()));
        console.log(any.x);
    "#;
    let h = hirs(&[("left", &left), ("right", &right), ("main", main)]);
    let pf = run(&h, false);
    let sites = x_sites(&pf, "main.ts");
    assert_eq!(sites.len(), 1, "{}", pf.dump_text());
    assert_ne!(sites[0].verdict, Verdict::T0Mono, "{}", pf.dump_text());
}

/// A module whose body throws, required inside a try (DESIGN SEEDE section 8):
/// its body runs in the require call, so what it throws reaches that catch,
/// by a static edge (the target's init guard) and by a deferred require (the
/// path registry runs the body).
#[test]
fn a_required_module_body_throw_reaches_the_catch() {
    let boom = "class Boom { z = 1; }\nexport var __perry_cjs_record;\n(function () { __perry_cjs_record = { exports: {}, loaded: false }; throw new Boom(); })();\n";
    let requirer = r#"
        import { __perry_cjs_record as __perry_cjs_rec_0 } from "./boom";
        function load() {
            const record = __perry_cjs_init(__perry_cjs_rec_0);
            return record.exports;
        }
        try { load(); } catch (e) { console.log((e as any).z); }
    "#;
    let h = hirs(&[("boom", boom), ("main", requirer)]);
    let pf = run(&h, false);
    let s = pf.sites.iter().find(|s| s.key == "z").expect("the z site");
    assert!(
        s.top || s.receivers.iter().any(|r| r.ends_with(":Boom")),
        "{:?}\n{}",
        s.receivers,
        pf.dump_text()
    );
    // Deferred: the requirer names the target only by a lazy import and
    // loads it through the path registry.
    let lazy = r#"
        import _lazyreq_0 from "./boom";
        function load() { return __perry_require_path_module("/boom.js"); }
        try { load(); } catch (e) { console.log((e as any).z); }
    "#;
    let mut h = hirs(&[("boom", boom), ("main", lazy)]);
    for imp in &mut h[1].1.imports {
        imp.is_deferred_require = true;
    }
    let pf = run(&h, false);
    let s = pf.sites.iter().find(|s| s.key == "z").expect("the z site");
    assert!(
        s.top || s.receivers.iter().any(|r| r.ends_with(":Boom")),
        "{:?}\n{}",
        s.receivers,
        pf.dump_text()
    );
}

/// The receivers of the `size` read on `require(<spec>).<key>`, where the
/// program's `require` is `createRequire`'s.
fn create_require_member(spec: &str, key: &str) -> Vec<String> {
    let src = format!(
        "import {{ createRequire }} from \"node:module\";\n\
         const r = createRequire(import.meta.url);\n\
         const v: any = r({spec}).{key};\n\
         console.log(v.size);\n"
    );
    site(&facts(&src), "size").receivers.clone()
}

/// A specifier that is no literal may name any builtin module: the loader
/// resolves builtins first, so `require(x)` is also every builtin module
/// object, and its member read resolves per key. Main-red: the result was
/// only ⊤, which holds no native function, and `createHook` (whose hooks run
/// at every async boundary) was never reached. Control: `resolve` names no
/// `createHook`.
#[test]
fn a_computed_require_is_any_builtin_module() {
    let hook = create_require_member("process.argv[2]", "createHook");
    assert!(hook.iter().any(|t| t.contains("createHook")), "{hook:?}");
    let resolve = create_require_member("process.argv[2]", "resolve");
    assert!(
        !resolve.iter().any(|t| t.contains("createHook")),
        "{resolve:?}"
    );
}

/// A literal builtin specifier names its own module object only.
#[test]
fn a_literal_builtin_require_is_its_module() {
    let hook = create_require_member("\"async_hooks\"", "createHook");
    assert!(hook.iter().any(|t| t.contains("createHook")), "{hook:?}");
    let fs = create_require_member("\"fs\"", "createHook");
    assert!(!fs.iter().any(|t| t.contains("createHook")), "{fs:?}");
}
