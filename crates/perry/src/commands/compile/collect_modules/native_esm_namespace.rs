//! Native ESM namespaces are ordinary compiled modules. Their exports use the
//! existing native import bindings, including the canonical CommonJS default,
//! and their namespace object uses the ordinary module layout and live getters.
//! No runtime loader, namespace registry, or extra identity cache is involved.

use std::collections::BTreeSet;
use std::fmt::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use swc_ecma_ast as ast;

pub(super) fn is_builtin(specifier: &str) -> bool {
    !perry_hir::is_bare_prefix_only_builtin(specifier)
        && perry_hir::is_node_builtin_module(specifier.strip_prefix("node:").unwrap_or(specifier))
}

/// One source module per canonical builtin in this compilation's generated
/// source directory. Both static namespaces and dynamic imports resolve here,
/// so the ordinary module init guard owns their identity and GC roots.
pub(super) fn materialize(cache_dir: &Path, specifier: &str) -> Result<PathBuf> {
    let module = specifier.strip_prefix("node:").unwrap_or(specifier);
    let filename = module
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let path = cache_dir
        .join("synthetic-modules")
        .join(format!("native-esm-{filename}.mjs"));
    let source = source(module);
    if std::fs::read_to_string(&path).ok().as_deref() != Some(source.as_str()) {
        std::fs::create_dir_all(path.parent().unwrap())
            .with_context(|| format!("create native namespace directory for {}", path.display()))?;
        std::fs::write(&path, source)
            .with_context(|| format!("write native namespace {}", path.display()))?;
    }
    Ok(path)
}

fn source(module: &str) -> String {
    // The public manifest already defines which bindings a static native named
    // import may expose. Forward those exact bindings, rather than copying the
    // mutable CommonJS object's properties or inventing a second export table.
    let names: BTreeSet<_> = perry_api_manifest::entries_for_module(module)
        .filter(|entry| perry_api_manifest::module_has_public_named_export(module, entry.name))
        .map(|entry| entry.name)
        .filter(|name| *name != "default")
        .collect();
    let specifier = serde_json::to_string(&format!("node:{module}")).unwrap();
    let mut source =
        format!("import __perry_default from {specifier};\nexport default __perry_default;\n");
    if !names.is_empty() {
        let names = names.into_iter().collect::<Vec<_>>().join(", ");
        writeln!(source, "import {{ {names} }} from {specifier};").unwrap();
        writeln!(source, "export {{ {names} }};").unwrap();
    }
    source
}

/// A native namespace import denotes the ESM namespace, while a default or
/// named import keeps its existing native binding. Split mixed declarations
/// (`import value, * as ns from 'fs'`) so each takes its ordinary path.
pub(super) fn rewrite_static(
    module: &ast::Module,
    cache_dir: &Path,
) -> Result<Option<ast::Module>> {
    let mut replacement = None;
    for (index, item) in module.body.iter().enumerate() {
        let ast::ModuleItem::ModuleDecl(ast::ModuleDecl::Import(import)) = item else {
            continue;
        };
        let source = import.src.value.as_str().unwrap_or("");
        if import.type_only
            || !is_builtin(source)
            || !import
                .specifiers
                .iter()
                .any(|s| matches!(s, ast::ImportSpecifier::Namespace(_)))
        {
            continue;
        }
        let path = materialize(cache_dir, source)?;
        let rewritten = replacement.get_or_insert_with(|| module.clone());
        let ast::ModuleItem::ModuleDecl(ast::ModuleDecl::Import(target)) =
            &mut rewritten.body[index]
        else {
            unreachable!()
        };
        let mut namespace = target.clone();
        namespace
            .specifiers
            .retain(|s| matches!(s, ast::ImportSpecifier::Namespace(_)));
        namespace.src.value = path.to_string_lossy().as_ref().into();
        namespace.src.raw = None;
        target
            .specifiers
            .retain(|s| !matches!(s, ast::ImportSpecifier::Namespace(_)));
        if target.specifiers.is_empty() {
            *target = namespace;
        } else {
            // Append the additional hoisted import without changing any of the
            // original declaration's specifiers or attributes.
            rewritten
                .body
                .push(ast::ModuleItem::ModuleDecl(ast::ModuleDecl::Import(
                    namespace,
                )));
        }
    }
    Ok(replacement)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_builtin_names_and_prefix_only_rule() {
        assert!(is_builtin("fs"));
        assert!(is_builtin("node:fs"));
        assert!(is_builtin("fs/promises"));
        assert!(is_builtin("node:test"));
        assert!(!is_builtin("test"));
        assert!(!is_builtin("fs/unknown"));
    }

    #[test]
    fn namespace_forwards_native_bindings_without_copying_commonjs() {
        let source = source("fs");
        assert!(source.contains("import __perry_default from \"node:fs\";"));
        assert!(source.contains("export default __perry_default;"));
        assert!(source.contains("readFileSync"));
        assert!(!source.contains("require("));
        assert!(!source.contains("Object.assign"));
    }

    #[test]
    fn static_namespaces_share_one_generated_module_and_keep_default_bindings() {
        let dir = tempfile::tempdir().unwrap();
        let ast = perry_parser::parse_typescript(
            "import fs, * as ns from 'fs'; import * as other from 'node:fs';",
            "native.ts",
        )
        .unwrap();
        let rewritten = rewrite_static(&ast, dir.path()).unwrap().unwrap();
        let imports: Vec<_> = rewritten
            .body
            .iter()
            .filter_map(|item| match item {
                ast::ModuleItem::ModuleDecl(ast::ModuleDecl::Import(import)) => Some(import),
                _ => None,
            })
            .collect();
        assert_eq!(imports.len(), 3);
        assert_eq!(imports[0].src.value.as_str(), Some("fs"));
        assert!(matches!(
            imports[0].specifiers[0],
            ast::ImportSpecifier::Default(_)
        ));
        assert_eq!(imports[1].src.value, imports[2].src.value);
        assert_eq!(
            std::fs::read_dir(dir.path().join("synthetic-modules"))
                .unwrap()
                .count(),
            1
        );
    }
}
