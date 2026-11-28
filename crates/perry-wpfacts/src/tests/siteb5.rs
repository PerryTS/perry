// SITEB5 (2026-11-28): rule M (a parent constructor's followed `this.m()`)
// for a class whose parent is a value the solve settles (an imported class).
// The by-name chain is the reference: the same program in one module.
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

fn show(tag: &str, pf: &crate::ProgramFacts) -> Vec<String> {
    let mut out = Vec::new();
    for class in [":Base", ":Kid"] {
        for t in pf.templates.iter().filter(|t| t.display.ends_with(class)) {
            out.push(format!("{class} fixed_layout={} prefix_stable={}", t.fixed_layout, t.prefix_stable));
            for f in &t.fields {
                out.push(format!(
                    "{class}.{} slot={:?} definite={} readonly={} wc={} wp={}",
                    f.key, f.slot, f.definite, f.readonly, f.writers_ctor, f.writers_post
                ));
            }
        }
    }
    for s in &pf.sites {
        if ["b", "c", "d"].contains(&s.key.as_str()) && !s.access.is_store() {
            out.push(format!(
                "site {} .{} @{} {:?}",
                pf.module_paths[s.module as usize], s.key, s.byte_offset, s.verdict
            ));
        }
    }
    for l in &out {
        eprintln!("SB5 {tag} {l}");
    }
    out
}

// Base's constructor follows `this.m()` into Base.m (rule M). Kid overrides
// m: its instances run Kid.m there, which observes `this` (b is 1) and does
// not store c. By name, rule M sees the override and takes the call as a
// leak: Kid.b has a writer after `this` is observed, c is not definite.
const BASE: &str = "class Base { constructor() { this.b = 1; this.m(); this.b = 2; this.d = 4; } m() { this.c = 3; } }\n";
const KID: &str = "export const seen = [];\nexport class Kid extends Base { m() { seen.push(this); } }\n";
const MAIN: &str = "const k = new Kid(); const q = new Base(); console.log(k.b, k.c, k.d, q.b, q.c, seen[0].b);\n";

fn kid_field(pf: &crate::ProgramFacts, k: &str) -> (bool, bool, usize) {
    let t = pf.templates.iter().find(|t| t.display.ends_with(":Kid")).expect("Kid");
    t.fields
        .iter()
        .find(|f| f.key == k)
        .map_or((false, false, 0), |f| (f.readonly, f.definite, f.writers_post))
}

#[test]
fn siteb5_a1_rule_m_value_parent() {
    let one = facts_at("a.ts", &format!("{BASE}{}{MAIN}", KID.replace("export ", "")));
    let by_name = show("name", &one);
    let mods = vec![
        ("main", format!("import {{ Kid, seen }} from './kid';\nimport {{ Base }} from './core';\n{MAIN}")),
        ("kid", format!("import {{ Base }} from './core';\n{KID}")),
        ("core", format!("export {BASE}")),
    ];
    let pf = whole(&mods);
    let by_value = show("value", &pf);
    let _ = (by_name, by_value);
    let (ro, _, wp) = kid_field(&pf, "b");
    let (ro_n, _, wp_n) = kid_field(&one, "b");
    eprintln!("SB5 kid.b value readonly={ro} wp={wp}; name readonly={ro_n} wp={wp_n}");
    assert!(!ro_n, "by name: Kid.b is written after Kid.m observed `this`");
    assert!(!ro, "by value: Kid.b is written after Kid.m observed `this`");
    // Kid.m stores no c: a Kid instance has no own c (base: definite, slot 1).
    let (_, def_c, _) = kid_field(&pf, "c");
    let (_, def_c_n, _) = kid_field(&one, "c");
    assert!(!def_c_n && !def_c, "Kid.c definite: name={def_c_n} value={def_c}");
}
