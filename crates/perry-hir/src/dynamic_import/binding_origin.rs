//! Resolve namespace entries to the module that owns each exported binding.

use crate::ir::{Export, ImportSpecifier, Module, Stmt};
use std::collections::HashSet;

/// #6304: where an exported name's value actually lives, after following
/// import bindings and re-export hops through the module graph.
pub(super) struct BindingOrigin {
    /// Module that owns the binding.
    pub(super) source_module: String,
    /// The name the binding has *in* `source_module`.
    pub(super) source_local: String,
    /// `Some(m)` when the binding is the module namespace of `m` rather than
    /// a plain value (`import * as X` / `export * as X`).
    pub(super) namespace_of: Option<String>,
}

/// True when `module` actually defines `name`, as opposed to importing or
/// re-exporting it. A definition stops origin resolution.
fn defines_local_binding(module: &Module, name: &str) -> bool {
    module.functions.iter().any(|f| f.name == name)
        || module.classes.iter().any(|c| c.name == name)
        || module.globals.iter().any(|g| g.name == name)
        || module.enums.iter().any(|e| e.name == name)
        // Module-scoped `const` / `let` declarations live as direct `Stmt::Let`
        // entries in `Module::init`, not in `Module::globals`. They are still
        // real local definitions and must stop a re-export origin walk.
        || module.init.iter().any(|stmt| {
            matches!(stmt, Stmt::Let { name: local, .. } if local == name)
        })
}

/// The import binding, if any, that `name` refers to in `module`.
/// Native imports are excluded because their source has no compiled HIR owner.
fn find_import_binding(module: &Module, name: &str) -> Option<(String, ImportBindingKind)> {
    for import in &module.imports {
        if import.type_only || import.runtime_erased || import.is_native {
            continue;
        }
        for spec in &import.specifiers {
            match spec {
                ImportSpecifier::Named { imported, local } if local == name => {
                    return Some((
                        import.source.clone(),
                        ImportBindingKind::Value(imported.clone()),
                    ));
                }
                ImportSpecifier::Default { local } if local == name => {
                    return Some((
                        import.source.clone(),
                        ImportBindingKind::Value("default".to_string()),
                    ));
                }
                ImportSpecifier::Namespace { local } if local == name => {
                    return Some((import.source.clone(), ImportBindingKind::Namespace));
                }
                _ => {}
            }
        }
    }
    None
}

enum ImportBindingKind {
    Value(String),
    Namespace,
}

/// Resolve `(module_name, local)` to the module that actually defines the
/// binding. Returns `None` when the chain does not move or leaves the compiled
/// module graph, preserving the caller's existing one-hop fallback.
pub(super) fn resolve_binding_origin<'a, F>(
    start_module: &str,
    start_local: &str,
    lookup: &F,
) -> Option<BindingOrigin>
where
    F: Fn(&str) -> Option<&'a Module>,
{
    let mut seen = HashSet::new();
    let origin = resolve_exported_binding(start_module, start_local, lookup, &mut seen)?;
    (origin.source_module != start_module
        || origin.source_local != start_local
        || origin.namespace_of.is_some())
    .then_some(origin)
}

/// Resolve one exported binding to its owner, including an `export *` barrel.
///
/// #7964: `leaf: export const v`, `barrel: export * from leaf`, and
/// `bridge: export { v } from barrel` must resolve to `leaf`, because the pure
/// barrel does not emit a `perry_fn_barrel__v` getter. Each star branch gets its
/// own cycle set; distinct successful owners are ambiguous and do not resolve.
fn resolve_exported_binding<'a, F>(
    module_name: &str,
    local: &str,
    lookup: &F,
    seen: &mut HashSet<(String, String)>,
) -> Option<BindingOrigin>
where
    F: Fn(&str) -> Option<&'a Module>,
{
    if !seen.insert((module_name.to_string(), local.to_string())) {
        return None;
    }
    let module = lookup(module_name)?;

    if defines_local_binding(module, local) {
        return Some(BindingOrigin {
            source_module: module_name.to_string(),
            source_local: local.to_string(),
            namespace_of: None,
        });
    }

    if let Some((source, kind)) = find_import_binding(module, local) {
        if lookup(&source).is_some() {
            return match kind {
                ImportBindingKind::Value(imported) => {
                    resolve_exported_binding(&source, &imported, lookup, seen)
                }
                ImportBindingKind::Namespace => Some(BindingOrigin {
                    source_module: source.clone(),
                    source_local: String::new(),
                    namespace_of: Some(source),
                }),
            };
        }
    }

    // `const _null = ...; export { _null as null }` maps the export name back
    // to the local binding before the walk continues. Zod exports both `null`
    // and `undefined` this way.
    for export in &module.exports {
        if let Export::Named {
            local: source,
            exported,
        } = export
        {
            if exported == local && source != local {
                return resolve_exported_binding(module_name, source, lookup, seen);
            }
        }
    }

    // Explicit cross-module exports take precedence over star exports.
    for export in &module.exports {
        match export {
            Export::ReExport {
                source,
                imported,
                exported,
            } if exported == local && lookup(source).is_some() => {
                return resolve_exported_binding(source, imported, lookup, seen);
            }
            Export::NamespaceReExport { source, name }
                if name == local && lookup(source).is_some() =>
            {
                return Some(BindingOrigin {
                    source_module: source.clone(),
                    source_local: String::new(),
                    namespace_of: Some(source.clone()),
                });
            }
            _ => {}
        }
    }

    if local == "default" {
        return None;
    }

    let mut resolved: Option<BindingOrigin> = None;
    for export in &module.exports {
        let Export::ExportAll { source } = export else {
            continue;
        };
        let mut branch_seen = seen.clone();
        let Some(candidate) = resolve_exported_binding(source, local, lookup, &mut branch_seen)
        else {
            continue;
        };
        if resolved.as_ref().is_some_and(|prior| {
            prior.source_module != candidate.source_module
                || prior.source_local != candidate.source_local
                || prior.namespace_of != candidate.namespace_of
        }) {
            return None;
        }
        resolved = Some(candidate);
    }
    resolved
}
