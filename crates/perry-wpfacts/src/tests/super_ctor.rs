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
    // DECISIONS 47: the solve sees a value parent unsettled, so a read of
    // the subclass's own key may be `undefined` (settle_parent_layouts).
    let k = &field(tmpl(&pf, "H"), "v").kind;
    assert!(
        !k.top
            && k.prims & prim::NUM != 0
            && k.prims & !(prim::NUM | prim::UNDEF) == 0
            && k.objs.is_empty(),
        "{k:?}
{}",
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
