//! Lowering coverage for Bun's callable unified SQL constructor (#9604).

use crate::ir::clear_current_module_source;

/// Bun.SQL returns a callable tagged-template client. `new` must keep the
/// native constructor's return value for named, namespace, and global Bun
/// spellings instead of wrapping it in a generic class instance.
#[test]
fn constructor_preserves_callable_native_client() {
    let source = r#"
        import { SQL } from "bun";
        import * as BunModule from "bun";

        const named = new SQL(":memory:");
        const namespaced = new BunModule.SQL(":memory:");
        const global = new Bun.SQL(":memory:");
        const rows = named`SELECT ${1} AS value`;
    "#;
    let module = perry_parser::parse_typescript(source, "bun_sql.ts").expect("source should parse");
    let hir = super::lower_module(&module, "test", "bun_sql.ts").expect("source should lower");
    clear_current_module_source();

    let dump = format!("{hir:#?}");
    assert_eq!(
        dump.matches("module: \"bun\"").count(),
        3,
        "every Bun.SQL constructor spelling must use native dispatch: {dump}"
    );
    assert_eq!(
        dump.matches("method: \"SQL\"").count(),
        3,
        "every constructor must preserve js_bun_sql_new's callable return: {dump}"
    );
    assert!(
        dump.contains("TaggedTemplateStrings"),
        "the returned client must remain usable as a template tag: {dump}"
    );
}
