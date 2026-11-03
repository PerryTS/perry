//! A module binding declared by an import and again by a local declaration is
//! an early SyntaxError, as in V8 ("Identifier 'x' has already been declared").

use perry_diagnostics::SourceCache;
use perry_hir::lower_module;
use perry_parser::parse_typescript_with_cache;

fn lower_src(src: &str) -> anyhow::Result<perry_hir::Module> {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(src, "import_redeclaration.ts", &mut cache)?;
    lower_module(&parsed.module, "test", "import_redeclaration.ts")
}

fn assert_redeclared(src: &str, name: &str) {
    let err = lower_src(src).expect_err("the redeclaration must be rejected");
    assert_eq!(
        err.to_string(),
        format!("SyntaxError: Identifier '{name}' has already been declared"),
        "source: {src}"
    );
}

#[test]
fn local_function_redeclaring_an_import_is_rejected() {
    assert_redeclared(
        "import { helper } from './m';\nfunction helper(): string { return 'local'; }\nhelper();",
        "helper",
    );
}

#[test]
fn every_local_binding_form_conflicts_with_an_import() {
    assert_redeclared("import a from './m';\nconst a = 1;", "a");
    assert_redeclared("import * as ns from './m';\nclass ns {}", "ns");
    assert_redeclared("import { b } from './m';\nlet [x, { b }] = [1, {}];", "b");
    assert_redeclared("import { c } from './m';\nif (true) { var c = 1; }", "c");
    assert_redeclared("import { d } from './m';\nexport function d() {}", "d");
    assert_redeclared("import { e } from './m';\nimport { x as e } from './n';", "e");
}

#[test]
fn bindings_that_declare_nothing_do_not_conflict() {
    lower_src(
        "import type { T } from './m';\nimport { type U, v } from './m';\n\
         function T(): void {}\nconst U = 1;\n\
         declare function v(x: number): void;\nfunction w(x: number): void;\nfunction w(x: any) {}\n\
         export { v };",
    )
    .expect("type-only imports, overload signatures and declare forms bind nothing");
    lower_src("import { f } from './m';\nfunction g() { var f = 1; let h = f; return h; }")
        .expect("a nested function's own bindings may shadow an import");
}
