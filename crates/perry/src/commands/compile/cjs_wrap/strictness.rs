//! Recognize a CommonJS source's original strict directive before wrapping.
use std::path::Path;
use swc_ecma_ast as ast;

pub(super) fn has_strict_directive(source: &str, source_path: &Path) -> bool {
    // Parse in a function body: return at CommonJS top level is legal. Remove
    // a hashbang before nesting it; the real wrapper neutralizes it as well.
    let source = if source.starts_with("#!") {
        source.split_once('\n').map_or("", |(_, body)| body)
    } else {
        source
    };
    let input = format!("function __cjs_directives__() {{\n{source}\n}}");
    let Ok(module) = perry_parser::parse_typescript(&input, &source_path.to_string_lossy()) else {
        return false;
    };
    let Some(ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Fn(function)))) = module.body.first()
    else {
        return false;
    };
    let Some(body) = &function.function.body else {
        return false;
    };
    for statement in &body.stmts {
        let ast::Stmt::Expr(expression) = statement else {
            break;
        };
        let ast::Expr::Lit(ast::Lit::Str(literal)) = expression.expr.as_ref() else {
            break;
        };
        // Cooked string contents would mistake "use\x20strict" for a directive.
        if matches!(
            literal.raw.as_ref().map(|raw| raw.as_ref()),
            Some("\"use strict\"") | Some("'use strict'")
        ) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_only_original_directive_prologues() {
        for source in [
            "'use strict'; module.exports = {};",
            "#! /usr/bin/env node\n/* header */ 'other';\n'use strict'\nreturn 1;",
            "\"use strict\"; return 1;",
        ] {
            assert!(
                has_strict_directive(source, Path::new("commonjs.cjs")),
                "{source}"
            );
        }
        for source in [
            "module.exports = {};",
            "'use\\x20strict'; module.exports = {};",
            "('use strict'); module.exports = {};",
            "'use strict'.trim(); module.exports = {};",
            "var x = 1; 'use strict'; module.exports = {};",
        ] {
            assert!(
                !has_strict_directive(source, Path::new("commonjs.cjs")),
                "{source}"
            );
        }
    }

    #[test]
    fn wrapped_strictness_preserves_body_offsets() {
        let path = Path::new("same-offset.cjs");
        let strict = "'use strict'; module.exports = function () { return this; };";
        let sloppy = "'use sloppy'; module.exports = function () { return this; };";
        let (strict_wrap, strict_offset) =
            super::super::wrap::wrap_commonjs_with_body_offset(strict, path, None, false, None);
        let (sloppy_wrap, sloppy_offset) =
            super::super::wrap::wrap_commonjs_with_body_offset(sloppy, path, None, false, None);
        assert!(strict_offset.is_some());
        assert_eq!(strict_offset, sloppy_offset);
        assert_eq!(strict_wrap.len(), sloppy_wrap.len());
        assert!(strict_wrap.contains("createRequire as __perry_cjs_strict_require"));
        assert!(sloppy_wrap.contains("createRequire as __perry_cjs_create_require"));
        for (wrapped, strict) in [(&strict_wrap, true), (&sloppy_wrap, false)] {
            let parsed = perry_parser::parse_typescript(wrapped, "same-offset.cjs").unwrap();
            let hir = perry_hir::lower_module(&parsed, "same-offset", "same-offset.cjs").unwrap();
            assert!(hir.is_commonjs_wrap());
            assert!(perry_codegen::module_is_cjs_wrapped(&hir));
            assert_eq!(hir.init_is_strict, strict);
        }
    }

    #[test]
    fn preserves_strictness_for_typescript_and_jsx() {
        for (source, path) in [
            (
                "'use strict'; const value: number = 1; module.exports = value;",
                "entry.cts",
            ),
            ("'use strict'; module.exports = () => <div/>;", "entry.jsx"),
            (
                "'use strict'; module.exports = (x: number) => <div>{x}</div>;",
                "entry.tsx",
            ),
        ] {
            assert!(has_strict_directive(source, Path::new(path)), "{path}");
        }
    }
}
