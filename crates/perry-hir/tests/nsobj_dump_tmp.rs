use perry_diagnostics::SourceCache;
use perry_parser::parse_typescript_with_cache;
#[test]
fn dump() {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(
        "namespace N { export const before = 1; }",
        "test.ts",
        &mut cache,
    )
    .unwrap();
    let m = perry_hir::lower_module(&parsed.module, "test", "test.ts").unwrap();
    println!("{:#?}", m.init);
}
