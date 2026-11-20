//! The construction entry order of a class chain (#8969): the one rule field
//! slots come from.

use super::{claimed_private_chain, construction_entries, private_entry_precedes};
use crate::lower_module;
use crate::{Class, Module};
use perry_diagnostics::SourceCache;
use perry_parser::parse_typescript_with_cache;

fn lower_src(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = parse_typescript_with_cache(&src, "test.ts", &mut cache)
                .expect("parse should succeed");
            lower_module(&parsed.module, "test", "test.ts").expect("lowering should succeed")
        })
        .expect("spawn")
        .join()
        .expect("join")
}

fn class<'m>(m: &'m Module, name: &str) -> &'m Class {
    m.classes.iter().find(|c| c.name == name).expect(name)
}

fn chain<'m>(m: &'m Module, names: &[&str]) -> Vec<&'m [crate::ClassField]> {
    names
        .iter()
        .map(|n| class(m, n).fields.as_slice())
        .collect()
}

/// The runtime appends `[parent, #ps, child]`: the parent's private claim
/// comes before the subclass defines its fields.
#[test]
fn a_parents_private_field_is_an_entry_before_the_subclass_fields() {
    let m = lower_src(
        "class P { parent = 1; #ps = 2; pt() { return this.#ps; } }\n\
         class C extends P { child = 3; }\n\
         console.log(new C().child);",
    );
    let names: Vec<&str> = construction_entries(chain(&m, &["P", "C"]))
        .map(|f| f.name.as_str())
        .collect();
    assert_eq!(names, ["parent", "#ps", "child"]);
    let fields = chain(&m, &["P", "C"]);
    let field = |n: &str| {
        construction_entries(fields.clone())
            .find(|f| f.name == n)
            .expect(n)
    };
    assert!(!private_entry_precedes(fields.clone(), field("parent")));
    assert!(private_entry_precedes(fields.clone(), field("child")));
    // The "birth keys + i" model is not this order: no claimed chain.
    let lookup = |n: &str| m.classes.iter().find(|c| c.name == n);
    assert!(claimed_private_chain(class(&m, "C"), &lookup).is_none());
    assert!(claimed_private_chain(class(&m, "P"), &lookup).is_some());
}

/// A base class: a public field declared after a private one follows it.
#[test]
fn a_public_field_after_a_private_one_follows_its_claim() {
    let m = lower_src(
        "class A { a = 1; #p = 2; b = 3; get() { return this.#p; } }\n\
         class L { a = 1; b = 3; #p = 2; get() { return this.#p; } }\n\
         console.log(new A().b, new L().b);",
    );
    let lookup = |n: &str| m.classes.iter().find(|c| c.name == n);
    let a = chain(&m, &["A"]);
    let b = construction_entries(a.clone())
        .find(|f| f.name == "b")
        .unwrap();
    assert!(private_entry_precedes(a.clone(), b));
    assert!(claimed_private_chain(class(&m, "A"), &lookup).is_none());
    // Every private entry last: the claimed layout holds.
    let l = chain(&m, &["L"]);
    let b = construction_entries(l.clone())
        .find(|f| f.name == "b")
        .unwrap();
    assert!(!private_entry_precedes(l, b));
    assert!(claimed_private_chain(class(&m, "L"), &lookup).is_some());
}
