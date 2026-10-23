//! Preserve a CommonJS source's explicit strict directive through the wrap.
use std::path::Path;
use swc_ecma_ast as ast;

pub(super) fn strict_prologue(source: &str, source_path: &Path) -> &'static str {
    // Parse in a function body: return at CommonJS top level is legal. Remove
    // a hashbang before nesting it; the real wrapper neutralizes it as well.
    let source = if source.starts_with("#!") {
        source.split_once('\n').map_or("", |(_, body)| body)
    } else {
        source
    };
    let input = format!("function __cjs_directives__() {{\n{source}\n}}");
    let Ok(module) = perry_parser::parse_typescript(&input, &source_path.to_string_lossy()) else {
        return "";
    };
    let Some(ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Fn(function)))) = module.body.first()
    else {
        return "";
    };
    let Some(body) = &function.function.body else {
        return "";
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
            return "\"use strict\";\n";
        }
    }
    ""
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
                !strict_prologue(source, Path::new("commonjs.cjs")).is_empty(),
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
                strict_prologue(source, Path::new("commonjs.cjs")).is_empty(),
                "{source}"
            );
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
            assert!(
                !strict_prologue(source, Path::new(path)).is_empty(),
                "{path}"
            );
        }
    }
}
