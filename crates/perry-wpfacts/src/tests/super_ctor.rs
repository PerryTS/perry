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
