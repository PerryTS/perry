//! PERFNOW38/39 (2026-11-20): every `new Proxy` site is its own Proxy, also
//! in a body the activation pass walks only once it is reached
//! (`Gen::walk_activated`). Such a body's calls count past the summaries'
//! `n_prog`; `origin_of` gave them no program site, so `birth_ctx` gave
//! every one the shared `BORN` context and all their Proxies one record:
//! one Proxy's [[GetOwnProperty]] ran another's `getOwnPropertyDescriptor`
//! trap (web tokens.ts: the style Proxy reached the dataset trap).
//! The Proxy-making functions here are reached only through a value the
//! first solve resolves (a property, a getter, an import), so their bodies
//! are walked after the summaries.
use super::*;

const TRAPS: &str = "function gopdTrap(t: any, k: any): any { return { value: 1, writable: true, enumerable: true, configurable: true }; }\n\
function styleDecl(): any { const api: any = { a: 1 }; return new Proxy(api, { get(t: any, k: any) { return api[k]; } }); }\n\
function dataset(): any { return new Proxy({} as any, { getOwnPropertyDescriptor: gopdTrap }); }\n";

fn lower(name: &str, src: &str) -> (String, perry_hir::Module) {
    let path = format!("{name}.ts");
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let mut hir = perry_hir::lower_module(&module, name, &path).expect("lowers");
    let mut specs = Vec::new();
    perry_hir::for_each_dynamic_import_mut(&mut hir, &mut |e| {
        if let perry_hir::Expr::DynamicImport { paths, arg, .. } = e {
            if let perry_hir::Expr::String(s) = arg.as_ref() {
                *paths = vec![s.clone()];
                specs.push(s.clone());
            }
        }
    });
    for s in specs {
        hir.imports.push(perry_hir::Import {
            source: s,
            specifiers: Vec::new(),
            is_native: false,
            module_kind: Default::default(),
            resolved_path: None,
            type_only: false,
            runtime_erased: false,
            is_dynamic: true,
            is_deferred: false,
            is_dynamic_target: false,
            is_deferred_require: false,
            is_adopted_require: false,
        });
    }
    for imp in &mut hir.imports {
        if let Some(n) = imp.source.strip_prefix("./") {
            imp.resolved_path = Some(format!("{}.ts", n.trim_end_matches(".js")));
        }
    }
    (path, hir)
}

/// Whether `gopdTrap` (module `tm`) can never run in the program `mods`.
fn inactive(mods: &[(&str, &str)], tm: &str) -> bool {
    let hirs: Vec<_> = mods.iter().map(|(n, s)| lower(n, s)).collect();
    let id = hirs
        .iter()
        .find(|(p, _)| p == &format!("{tm}.ts"))
        .and_then(|(_, h)| h.functions.iter().find(|f| f.name == "gopdTrap"))
        .expect("gopdTrap")
        .id;
    let list: Vec<(String, &perry_hir::Module)> =
        hirs.iter().map(|(p, h)| (p.clone(), h)).collect();
    let cfg = FactsConfig {
        max_work: Some(50_000_000),
        ..FactsConfig::default()
    };
    let pf = crate::compute(&list, &cfg);
    assert!(pf.body_reach.complete);
    pf.body_reach
        .inactive(&(format!("{tm}.ts"), id, String::new()))
}

/// `s` and `d` made by `make` (the Proxy-making functions reached late).
fn late(make: &str, own: &str) -> bool {
    let src = format!("{TRAPS}{make}const key = String(process.argv[2]);\nconsole.log(d.x);\nconsole.log({own});\n");
    inactive(&[("t", &src)], "t")
}

const BY_PROPERTY: &str = "const mk: any = { s: styleDecl, d: dataset };\nconst s: any = mk.s();\nconst d: any = mk.d();\n";
const BY_GETTER: &str = "class H { get s(): any { return styleDecl(); } get d(): any { return dataset(); } }\nconst h = new H();\nconst s: any = h.s;\nconst d: any = h.d;\n";

// [[GetOwnProperty]] of the Proxy without the trap never runs the other
// Proxy's trap, whether its maker is reached through a property or a getter.
#[test]
fn late_walked_proxies_are_two_records() {
    for make in [BY_PROPERTY, BY_GETTER] {
        for own in ["Object.hasOwn(s, key)", "s.hasOwnProperty(key)"] {
            assert!(
                late(make, own),
                "other Proxy's gOPD trap ran for {own} in\n{make}"
            );
        }
    }
}

// The same through an imported maker.
#[test]
fn late_walked_proxies_of_another_module_are_two_records() {
    let lib = format!("{TRAPS}export function mkS(): any {{ return styleDecl(); }}\nexport function mkD(): any {{ return dataset(); }}\n");
    let main = "import { mkS, mkD } from \"./tok\";\nconst s: any = mkS();\nconst d: any = mkD();\nconst key = String(process.argv[2]);\nconsole.log(d.x);\nconsole.log(Object.hasOwn(s, key));\n";
    assert!(inactive(&[("tok", &lib), ("main", main)], "tok"));
}

// The control: on the Proxy with the trap, the trap runs.
#[test]
fn own_prop_of_the_trapping_proxy_runs_its_trap() {
    for make in [BY_PROPERTY, BY_GETTER] {
        assert!(!late(make, "Object.hasOwn(d, key)"), "{make}");
    }
}
