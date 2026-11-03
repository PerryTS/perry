//! Per unit, a class whose class object is relinked as another class's
//! parent (`Object.setPrototypeOf(B, A)`, `Reflect.setPrototypeOf`,
//! `B.__proto__ = A`): `B`'s `super()` runs `A`'s constructor on a `B`
//! instance. The whole program sees that receiver; per unit, `A`'s unit
//! must not prove `this` of `A`'s constructor any stronger.

use super::*;
use crate::bindings::BindingFacts;

fn hirs_of(mods: &[(&str, &str)]) -> Vec<(String, perry_hir::Module)> {
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

/// The facts of `this` of class `class`'s constructor (module `module`),
/// per unit and whole program.
fn ctor_this(
    mods: &[(&str, &str)],
    module: &str,
    class: &str,
) -> [(ProgramFacts, Option<BindingFacts>); 2] {
    let hirs = hirs_of(mods);
    let list: Vec<(String, &perry_hir::Module)> =
        hirs.iter().map(|(p, h)| (p.clone(), h)).collect();
    let path = format!("{module}.ts");
    let (_, hir) = hirs.iter().find(|(p, _)| *p == path).expect("module");
    let id = hir
        .classes
        .iter()
        .find(|c| c.name == class)
        .and_then(|c| c.constructor.as_ref())
        .expect("a class with a constructor")
        .id;
    let cfg = FactsConfig {
        max_work: Some(50_000_000),
        ..FactsConfig::default()
    };
    [crate::compute_units(&list, &cfg), compute(&list, &cfg)].map(|pf| {
        let b = pf
            .bindings_for(&path)
            .and_then(|mb| mb.this_of.iter().find(|(f, _)| *f == id))
            .map(|(_, b)| b.clone());
        (pf, b)
    })
}

/// What a store `this.<key>` in the constructor relies on: `this` holds
/// only instances of classes whose layout gives `key` a definite slot (a
/// slot store, no receiver check).
fn slot_store(pf: &ProgramFacts, this: &BindingFacts, key: &str) -> bool {
    !this.other
        && !this.class_ids.is_empty()
        && this.class_ids.iter().all(|&c| {
            pf.templates.iter().any(|t| {
                t.class_id == Some(c)
                    && t.prefix_stable
                    && t.fields
                        .iter()
                        .any(|f| f.key == key && f.slot.is_some() && f.definite)
            })
        })
}

/// Per unit claims no more of the constructor's `this` than the whole
/// program: a `this.parentSaw` store per unit is a slot store only where it
/// is one in the whole program.
fn assert_not_stronger(mods: &[(&str, &str)], module: &str, class: &str) {
    let [(pu, u), (pw, w)] = ctor_this(mods, module, class);
    let w = w.expect("whole program has the constructor");
    // The test can fail: the whole program sees the relinked subclass's
    // instance as the constructor's receiver.
    assert!(
        w.other || w.class_ids.len() > 1,
        "whole program: {w:?}\n{}",
        pw.dump_text()
    );
    // `super()` of a class per unit follows its declared parent only where
    // the whole program proves it unrelinked.
    let stronger: Vec<_> = pu
        .super_unrelinked
        .difference(&pw.super_unrelinked)
        .collect();
    assert!(
        stronger.is_empty(),
        "{mods:?}: per unit claims classes {stronger:?} unrelinked\n{}",
        pu.dump_text()
    );
    let Some(u) = u else { return };
    assert!(
        !slot_store(&pu, &u, "parentSaw") || slot_store(&pw, &w, "parentSaw"),
        "{mods:?}: per unit {u:?} is stronger than whole {w:?}\n{}",
        pu.dump_text()
    );
}

const ACTUAL: &str = "class ActualParent { constructor() { this.parentSaw = Object.keys(this).join(\",\"); this.dynamic = 1; } }";

const RELINKS: [&str; 3] = [
    "Object.setPrototypeOf(OrderedDefault, ActualParent);",
    "Reflect.setPrototypeOf(OrderedDefault, ActualParent);",
    "(OrderedDefault as any).__proto__ = ActualParent;",
];

const CTORS: [&str; 2] = ["", "constructor() { super(); }"];

#[test]
fn a_relinked_parent_constructor_runs_on_the_subclass_instance_in_one_module() {
    for relink in RELINKS {
        for ctor in CTORS {
            let src = format!(
                "class DeclaredParent {{}}\n{ACTUAL}\nclass OrderedDefault extends DeclaredParent {{ own = 2; {ctor} }}\n{relink}\nconst o: any = new OrderedDefault();\nconsole.log(o.parentSaw, Object.keys(o).join(\",\"));\nconst ap: any = new ActualParent();\nconsole.log(ap.parentSaw, ap.dynamic);"
            );
            assert_not_stronger(&[("t", &src)], "t", "ActualParent");
        }
    }
}

#[test]
fn a_relinked_parent_of_another_module_runs_on_the_subclass_instance() {
    let a = format!(
        "export {ACTUAL}\nconst ap: any = new ActualParent();\nconsole.log(ap.parentSaw, ap.dynamic);"
    );
    for relink in RELINKS {
        for ctor in CTORS {
            // The subclass and the relink in the importing module.
            let m = format!(
                "import {{ ActualParent }} from \"./a\";\nclass DeclaredParent {{}}\nclass OrderedDefault extends DeclaredParent {{ own = 2; {ctor} }}\n{relink}\nconst o: any = new OrderedDefault();\nconsole.log(o.parentSaw);"
            );
            assert_not_stronger(&[("a", &a), ("m", &m)], "a", "ActualParent");
            // The subclass in a third module, the relink beside the `new`.
            let b = format!(
                "class DeclaredParent {{}}\nexport class OrderedDefault extends DeclaredParent {{ own = 2; {ctor} }}"
            );
            let m = format!(
                "import {{ ActualParent }} from \"./a\";\nimport {{ OrderedDefault }} from \"./b\";\n{relink}\nconst o: any = new OrderedDefault();\nconsole.log(o.parentSaw);"
            );
            assert_not_stronger(&[("a", &a), ("b", &b), ("m", &m)], "a", "ActualParent");
            // The relink in the parent's own module, of another module's class.
            let a2 = format!(
                "import {{ OrderedDefault }} from \"./b\";\nexport {ACTUAL}\n{relink}\nconst ap: any = new ActualParent();\nconsole.log(ap.parentSaw, ap.dynamic);"
            );
            let m = "import { OrderedDefault } from \"./b\";\nimport \"./a\";\nconst o: any = new OrderedDefault();\nconsole.log(o.parentSaw);";
            assert_not_stronger(&[("b", &b), ("a", &a2), ("m", m)], "a", "ActualParent");
        }
    }
}

#[test]
fn a_relinked_parent_in_one_unit_runs_on_the_subclass_instance() {
    // The two classes name each other, so they are one declaration unit,
    // and the relink runs in the subclass's static block, in that unit.
    for relink in RELINKS {
        let src = format!(
            "class DeclaredParent {{}}\nclass ActualParent {{ constructor() {{ this.parentSaw = Object.keys(this).join(\",\"); this.dynamic = 1; }} sub() {{ return OrderedDefault; }} }}\nclass OrderedDefault extends DeclaredParent {{ own = 2; constructor() {{ super(); }} sup() {{ return ActualParent; }} static {{ {relink} }} }}\nconst o: any = new OrderedDefault();\nconst ap: any = new ActualParent();\nconsole.log(o.parentSaw, ap.parentSaw, ap.dynamic);"
        );
        assert_not_stronger(&[("t", &src)], "t", "ActualParent");
    }
}
