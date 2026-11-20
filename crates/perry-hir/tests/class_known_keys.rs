//! THR1: a computed class key the compiler knows is an ordinary member, and
//! only an evaluated key counts as "a key the compiler cannot name".

use perry_diagnostics::SourceCache;
use perry_hir::{lower_module, Class, Module};
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

#[test]
fn literal_keys_are_named_members() {
    let m = lower_src(
        r#"class A { ["foo"]() { return 1; } [0]() { return 2; } [1e3]() { return 3; }
           get ["g"]() { return 4; } set ["g"](v) {} bar() { return 5; } }
           new A();"#,
    );
    let a = class(&m, "A");
    assert!(
        a.computed_members.is_empty(),
        "{:?}",
        a.computed_members.len()
    );
    let names: Vec<_> = a.methods.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["foo", "0", "1000", "bar"]);
    assert!(a.getters.iter().any(|(n, _)| n == "g"));
    assert!(a.setters.iter().any(|(n, _)| n == "g"));
    assert!(!a.has_evaluated_members());
}

#[test]
fn well_known_and_registered_symbol_keys_are_known() {
    let m = lower_src(
        r#"class V { x = 0;
             *[Symbol.iterator]() { yield this.x; }
             [Symbol.toPrimitive](h) { return 1; }
             [Symbol.asyncIterator]() { return null; }
             [Symbol.dispose]() {}
             [Symbol.for("k")]() { return 2; }
             add() { return this.x; } }
           new V();"#,
    );
    let v = class(&m, "V");
    let known: Vec<_> = v
        .computed_members
        .iter()
        .filter_map(|c| c.known_symbol())
        .collect();
    assert_eq!(
        known,
        [
            "@@__perry_wk_iterator",
            "@@__perry_wk_toPrimitive",
            "@@__perry_wk_asyncIterator",
            "@@__perry_wk_dispose",
            "k"
        ]
    );
    assert!(!v.has_evaluated_members());
    // No rename to an own string key, and no source-order copy of `add`.
    assert!(v
        .methods
        .iter()
        .all(|f| !f.name.starts_with("@@") && !f.name.starts_with("__perry")));
    assert!(v.methods.iter().any(|f| f.name == "add"));
    assert_eq!(v.computed_members.len(), 5);
}

#[test]
fn evaluated_key_is_evaluated() {
    let m = lower_src(
        r#"const k = String(Math.random() > 2 ? "a" : "b");
           class E { [k]() { return 1; } after() { return 2; } }
           class S { set [k](v) {} }
           new E(); new S();"#,
    );
    let e = class(&m, "E");
    assert!(e.has_evaluated_members());
    assert!(!e.has_evaluated_accessor());
    assert!(class(&m, "S").has_evaluated_accessor());
}
