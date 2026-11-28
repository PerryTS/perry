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

/// The field rows of `class` (definite, readonly, writers), by key.
fn rows(tag: &str, pf: &crate::ProgramFacts, class: &str) -> Vec<String> {
    let mut out = Vec::new();
    for t in pf.templates.iter().filter(|t| t.display.ends_with(class)) {
        for f in &t.fields {
            out.push(format!(
                "{class}.{} definite={} readonly={} wc={} wp={}",
                f.key, f.definite, f.readonly, f.writers_ctor, f.writers_post
            ));
        }
    }
    out.sort();
    for l in &out {
        eprintln!("SB6 {tag} {l}");
    }
    out
}

fn field(pf: &crate::ProgramFacts, class: &str, k: &str) -> (bool, bool, usize) {
    let t = pf.templates.iter().find(|t| t.display.ends_with(class)).expect("class");
    t.fields
        .iter()
        .find(|f| f.key == k)
        .map_or((false, false, 0), |f| (f.readonly, f.definite, f.writers_post))
}

// Au's constructor follows `this.pre = ..` into Media's setter (rule S).
// Media's parent is the imported Root: before the solve Media has no parent
// link. Media is Au's own parent, so it is not below Au.
const ROOT: &str = "class Root { constructor() { this.r = 1; } }\n";
const MEDIA: &str = "export class Media extends Root { get pre() { return 0; } set pre(v) { this.c = v; } }\n\
export class Au extends Media { constructor() { super(); this.b = 1; this.pre = 3; this.b = 2; this.d = 4; } }\n";
const KID: &str = "export const seen = [];\nexport class Kid extends Au { get pre() { return 0; } set pre(v) { seen.push(this); } }\n";
const MAIN_A: &str = "const a = new Au(); console.log(a.b, a.c, a.d, a.r);\n";
const MAIN_B: &str = "const a = new Au(); const k = new Kid(); console.log(a.b, a.c, a.d, k.b, k.c, k.d, seen[0].b);\n";

#[test]
fn siteb6_a_own_chain_value_parent_precise() {
    let one = facts_at("a.ts", &format!("{ROOT}{}{MAIN_A}", MEDIA.replace("export ", "")));
    let mods = vec![
        ("main", format!("import {{ Au }} from './media';\n{MAIN_A}")),
        ("media", format!("import {{ Root }} from './core';\n{MEDIA}")),
        ("core", format!("export {ROOT}")),
    ];
    let pf = whole(&mods);
    let n = rows("name", &one, ":Au");
    let v = rows("value", &pf, ":Au");
    let (ro, def, _) = field(&pf, ":Au", "c");
    assert!(def, "Au.c (Media's setter, followed) is definite by value too");
    let (ro_b, _, _) = field(&pf, ":Au", "b");
    let (ro_bn, _, _) = field(&one, ":Au", "b");
    assert_eq!(ro_b, ro_bn, "Au.b readonly: value={ro_b} name={ro_bn}");
    let _ = ro;
    // Media's keys come from its own constructor chain (Root's r): only the
    // rows rule M decides are compared.
    let pick = |r: &Vec<String>| -> Vec<String> {
        r.iter().filter(|l| [".b ", ".c ", ".d "].iter().any(|k| l.contains(k))).cloned().collect()
    };
    assert_eq!(pick(&v), pick(&n), "Au rows by value equal by name");
}

// Kid's parent is the imported Au and Kid overrides `pre`: before the solve
// Kid may be below Au, so Au's followed setter is taken as a leak there, as
// by name (Kid's instances run Kid's setter, which observes `this`).
#[test]
fn siteb6_b_value_linked_override_refused() {
    let one = facts_at(
        "a.ts",
        &format!("{ROOT}{}{}{MAIN_B}", MEDIA.replace("export ", ""), KID.replace("export ", "")),
    );
    let mods = vec![
        ("main", format!("import {{ Au }} from './media';\nimport {{ Kid, seen }} from './kid';\n{MAIN_B}")),
        ("kid", format!("import {{ Au }} from './media';\n{KID}")),
        ("media", format!("import {{ Root }} from './core';\n{MEDIA}")),
        ("core", format!("export {ROOT}")),
    ];
    let pf = whole(&mods);
    let _ = rows("name", &one, ":Kid");
    let _ = rows("value", &pf, ":Kid");
    let (ro, _, _) = field(&pf, ":Kid", "b");
    let (ro_n, _, _) = field(&one, ":Kid", "b");
    assert!(!ro_n, "by name: Kid.b is written after Kid's setter observed `this`");
    assert!(!ro, "by value: Kid.b is written after Kid's setter observed `this`");
    let (_, def_c, _) = field(&pf, ":Kid", "c");
    let (_, def_c_n, _) = field(&one, ":Kid", "c");
    assert!(!def_c_n && !def_c, "Kid.c definite: name={def_c_n} value={def_c}");
}
