//! `super(...)` into a realm constructor (ECMA-262 13.3.7.1 SuperCall,
//! 10.1.13 OrdinaryCreateFromConstructor): the parent's declared construct
//! effects run on the instance under construction, whose prototype chain is
//! the program subclass's; a realm parent that declares none is an explicit
//! E21 with its reason.
use super::*;
use crate::model::prim;

#[test]
fn a_declared_realm_parent_constructs_the_instance_itself() {
    let pf = facts(
        "class T extends EventTarget { q = 1; }\nclass H { v: any = 0; }\nconst t = new T();\nconst h = new H();\nh.v = t.q;\nconsole.log(h.v);\n",
    );
    let t = tmpl(&pf, "T");
    assert!(t.hard.is_none(), "{:?}\n{}", t.hard, pf.dump_text());
    assert!(t.escape.is_none(), "{:?}\n{}", t.escape, pf.dump_text());
    // The realm parent is linked at generation time (`Model::realm_parent`):
    // its declared construct's fields are the layout prefix, so the
    // subclass's own field is definite in the solve.
    let k = &field(tmpl(&pf, "H"), "v").kind;
    assert!(
        !k.top && k.prims == prim::NUM && k.objs.is_empty(),
        "{k:?}\n{}",
        pf.dump_text()
    );
}

#[test]
fn a_realm_parent_without_a_construct_declaration_is_e21_with_its_reason() {
    let pf =
        facts("class M extends Map<any, any> { q = 1; }\nconst m = new M();\nconsole.log(m);\n");
    let t = tmpl(&pf, "M");
    let h = t.hard.as_deref().unwrap_or_default();
    assert!(
        h.starts_with("E21") && h.contains("declares no construct effects"),
        "{h:?}\n{}",
        pf.dump_text()
    );
}

#[test]
fn an_extends_value_that_is_not_the_linked_realm_parent_is_e11_with_its_reason() {
    let pf = facts(
        "(globalThis as any).EventTarget = class Q { };\nclass T extends EventTarget { q = 1; }\nconst t = new T();\nconsole.log(t.q);\n",
    );
    let h = tmpl(&pf, "T").hard.as_deref().unwrap_or_default();
    assert!(
        h.starts_with("E11") && h.contains("linked realm constructor"),
        "{h:?}\n{}",
        pf.dump_text()
    );
}

/// A heritage name links a realm parent only when it statically resolves to
/// the global binding: a module-level, local or imported binding of the same
/// name shadows it, and the class takes the program-binding parent path,
/// exactly as the same program with an unshadowing name (`Base`) does.
fn shadowed_parent_is_the_program_binding(mods: &[(&str, &str)]) {
    let control: Vec<(String, String)> = mods
        .iter()
        .map(|(n, s)| (n.to_string(), s.replace("EventTarget", "Base")))
        .collect();
    let control: Vec<(&str, &str)> = control
        .iter()
        .map(|(n, s)| (n.as_str(), s.as_str()))
        .collect();
    let (pf, pc) = (facts_mods(mods), facts_mods(&control));
    let t = tmpl(&pf, "T");
    assert!(
        t.hard.is_none() && t.escape.is_none(),
        "{:?} {:?}\n{}",
        t.hard,
        t.escape,
        pf.dump_text()
    );
    let k = &field(tmpl(&pf, "H"), "v").kind;
    let kc = &field(tmpl(&pc, "H"), "v").kind;
    assert!(
        !k.top && k.prims == kc.prims && k.prims & prim::NUM != 0 && k.objs.is_empty(),
        "{k:?} vs {kc:?}\n{}",
        pf.dump_text()
    );
}

fn facts_mods(mods: &[(&str, &str)]) -> ProgramFacts {
    let h: Vec<(String, perry_hir::Module)> = mods
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
    let list: Vec<(String, &perry_hir::Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    crate::compute(&list, &cfg)
}

#[test]
fn a_module_level_binding_shadowing_a_realm_ctor_name_links_no_realm_parent() {
    shadowed_parent_is_the_program_binding(&[(
        "t",
        "const EventTarget = class { p = 2; };\nclass T extends EventTarget { q = 1; }\nclass H { v: any = 0; }\nconst t = new T();\nconst h = new H();\nh.v = t.p;\nconsole.log(h.v);\n",
    )]);
}

#[test]
fn a_local_binding_shadowing_a_realm_ctor_name_links_no_realm_parent() {
    shadowed_parent_is_the_program_binding(&[(
        "t",
        "class H { v: any = 0; }\nconst h = new H();\nfunction f() {\n  const EventTarget = class { p = 2; };\n  class T extends EventTarget { q = 1; }\n  const t = new T();\n  h.v = t.p;\n}\nf();\nconsole.log(h.v);\n",
    )]);
}

#[test]
fn an_imported_binding_shadowing_a_realm_ctor_name_links_no_realm_parent() {
    shadowed_parent_is_the_program_binding(&[
        ("a", "export const EventTarget = class { p = 2; };\n"),
        (
            "main",
            "import { EventTarget } from \"./a\";\nclass T extends EventTarget { q = 1; }\nclass H { v: any = 0; }\nconst t = new T();\nconst h = new H();\nh.v = t.p;\nconsole.log(h.v);\n",
        ),
    ]);
}
