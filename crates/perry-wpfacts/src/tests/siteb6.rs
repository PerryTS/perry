// SITEB6 (2026-11-28): rule M before the solve for a class whose own chain
// ends in a parent value not yet settled (`extends v`, v imported). That
// class is the class itself or one of its ancestors: it is never below it,
// whatever the value settles to (a prototype chain has no cycle). A class
// with its own value link elsewhere may still be below it. The by-name
// chain (the same program in one module) is the reference.
use super::*;
use perry_hir::Module;

fn facts_at(path: &str, src: &str) -> crate::ProgramFacts {
    let module = perry_parser::parse_typescript(src, path).expect("parses");
    let hir = perry_hir::lower_module(&module, "a", path).expect("lowers");
    let h = vec![(path.to_string(), hir)];
    let list: Vec<(String, &Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    let cfg = FactsConfig {
        unit_io: true,
        ..Default::default()
    };
    crate::compute(&list, &cfg)
}

fn whole(mods: &[(&str, String)]) -> crate::ProgramFacts {
    let hirs: Vec<(String, Module)> = mods
        .iter()
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
        .collect();
    let cfg = FactsConfig {
        unit_io: true,
        ..Default::default()
    };
    let list: Vec<(String, &Module)> = hirs.iter().map(|(p, m)| (p.clone(), m)).collect();
    crate::compute(&list, &cfg)
}

/// The verdicts of the store sites `.pre = ..` in module `main`.
fn pre_stores(tag: &str, pf: &crate::ProgramFacts) -> Vec<String> {
    let mut out = Vec::new();
    for s in &pf.sites {
        let m = &pf.module_paths[s.module as usize];
        if s.key == "pre" && s.access.is_store() && (m.ends_with("a.ts") || m.ends_with("main.ts")) {
            out.push(format!("{:?}", s.verdict));
        }
    }
    for l in &out {
        eprintln!("SB6 {tag} store .pre {l}");
    }
    out
}

// Au's constructor follows `this.pre = ..` into Media's setter (rule S).
// Media's parent is the imported Root: before the solve Media has no parent
// link. Media is Au's own parent, so it is not below Au. Root's constructor
// leaks `this`, so no phase is recomputed after the solve (rule C): the
// verdict is the one the solve ran with (base: "an instance may own the key").
const ROOT: &str = "class Root { constructor() { this.r = 1; reg.push(this); } }\n";
const MEDIA: &str = "export class Media extends Root { get pre() { return this.at(); } set pre(v) { this.put(v); } at() { return 0; } put(v) { } }\n\
export class Au extends Media { constructor() { super(); this.pre = 3; this.z = 2; } }\n";
const KID: &str = "export const seen = [];\nexport class Kid extends Au { get pre() { return 0; } set pre(v) { seen.push(this); } }\n";
const MAIN_A: &str = "const a = new Au(); a.pre = 5; console.log(a.pre, a.r);\n";
const MAIN_B: &str = "const a = new Au(); a.pre = 5; const k = new Kid(); console.log(a.pre, k.pre, seen.length);\n";

#[test]
fn siteb6_a_own_chain_value_parent_precise() {
    let one = facts_at("a.ts", &format!("const reg = [];\n{ROOT}{}{MAIN_A}", MEDIA.replace("export ", "")));
    let mods = vec![
        ("main", format!("import {{ Au }} from './media';\n{MAIN_A}")),
        ("media", format!("import {{ Root }} from './core';\n{MEDIA}")),
        ("core", format!("export const reg = [];\nexport {ROOT}")),
    ];
    let pf = whole(&mods);
    let n = pre_stores("name", &one);
    let v = pre_stores("value", &pf);
    assert_eq!(v.last(), n.last(), "main's a.pre store: by value equals by name");
    assert_eq!(v.last().map(String::as_str), Some("AccessorSet"), "a.pre finds Media's setter");
}

// Kid's parent is the imported Au and Kid overrides `pre`: before the solve
// Kid may be below Au, so Au's followed setter is taken as a leak there, as
// by name (Kid's instances run Kid's setter, which observes `this`).
#[test]
fn siteb6_b_value_linked_override_refused() {
    let one = facts_at(
        "a.ts",
        &format!("const reg = [];\n{ROOT}{}{}{MAIN_B}", MEDIA.replace("export ", ""), KID.replace("export ", "")),
    );
    let mods = vec![
        ("main", format!("import {{ Au }} from './media';\nimport {{ Kid, seen }} from './kid';\n{MAIN_B}")),
        ("kid", format!("import {{ Au }} from './media';\n{KID}")),
        ("media", format!("import {{ Root }} from './core';\n{MEDIA}")),
        ("core", format!("export const reg = [];\nexport {ROOT}")),
    ];
    let pf = whole(&mods);
    let n = pre_stores("name", &one);
    let v = pre_stores("value", &pf);
    assert_eq!(v.last(), n.last(), "main's a.pre store: by value equals by name");
    assert!(v.last().is_some_and(|l| l.starts_with("No(")), "a.pre stays refused: {v:?}");
}
