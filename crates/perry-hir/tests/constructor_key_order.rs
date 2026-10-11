//! Constructor stores must not turn into observable field definitions (#12327).
use perry_diagnostics::SourceCache;
use perry_hir::lower_module;
use perry_parser::parse_typescript_with_cache;

#[test]
fn constructor_stores_do_not_declare_fields() {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(
        "class Cell { declared = 0; constructor(f) { this.before(); if(f) this.b=2; this.a=1; this.a=3; } before() { this.first=0; } } class Child extends Cell { constructor(f) { super(f); this.child=4; } }",
        "test.ts", &mut cache,
    ).unwrap();
    let module = lower_module(&parsed.module, "test", "test.ts").unwrap();
    let cell = module.classes.iter().find(|c| c.name == "Cell").unwrap();
    assert_eq!(
        cell.fields
            .iter()
            .filter(|f| f.origin == perry_hir::ClassFieldOrigin::Definition)
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>(),
        ["declared"]
    );
    let child = module.classes.iter().find(|c| c.name == "Child").unwrap();
    assert!(child
        .fields
        .iter()
        .all(|f| f.origin == perry_hir::ClassFieldOrigin::ConstructorStore));
    assert_eq!(child.fields[0].name, "child");
    let reserved: Vec<_> = cell
        .fields
        .iter()
        .filter(|f| f.origin == perry_hir::ClassFieldOrigin::ConstructorStore)
        .map(|f| f.name.as_str())
        .collect();
    assert_eq!(reserved, ["a"]);
    assert!(!cell.constructor.as_ref().unwrap().body.is_empty());
    assert!(!child.constructor.as_ref().unwrap().body.is_empty());
}
