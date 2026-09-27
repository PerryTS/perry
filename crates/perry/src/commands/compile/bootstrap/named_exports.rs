use super::CompilationContext;
use anyhow::{bail, Result};
use perry_hir::{Export, ImportSpecifier, ModuleKind};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Availability {
    Present,
    Absent,
    // Builtin/runtime-backed exports have no complete HIR export list.
    Unknown,
}

fn availability(
    ctx: &mut CompilationContext,
    path: &Path,
    name: &str,
    seen: &mut HashSet<(PathBuf, String)>,
) -> Availability {
    if !seen.insert((path.to_path_buf(), name.to_owned())) {
        return Availability::Absent;
    }
    let Some(module) = ctx.native_modules.get(path) else {
        return Availability::Unknown;
    };
    let exports = module.exports.clone();
    let mut result = Availability::Absent;
    for export in exports {
        let (source, imported) = match export {
            Export::Named { exported, .. } if exported == name => return Availability::Present,
            Export::NamespaceReExport { name: exported, .. } if exported == name => {
                return Availability::Present;
            }
            Export::ReExport {
                source,
                imported,
                exported,
            } if exported == name => (source, imported),
            Export::ExportAll { source } if name != "default" => (source, name.to_owned()),
            _ => continue,
        };
        let found = match super::super::cached_resolve_import(&source, path, ctx) {
            Some((target, ModuleKind::NativeCompiled)) => {
                availability(ctx, &target, &imported, seen)
            }
            _ => Availability::Unknown,
        };
        match found {
            Availability::Present => return found,
            Availability::Unknown => result = found,
            Availability::Absent => {}
        }
    }
    result
}

/// Reject statically absent named exports while module/source context is still
/// available, instead of inventing a perry_fn symbol that fails in the linker.
pub(super) fn enforce(ctx: &mut CompilationContext) -> Result<()> {
    let mut edges = Vec::new();
    for (importer, module) in &ctx.native_modules {
        for import in &module.imports {
            if import.type_only
                || import.runtime_erased
                || import.is_dynamic
                || import.is_native
                || import.is_adopted_require
                || import.module_kind != ModuleKind::NativeCompiled
            {
                continue;
            }
            let Some(target) = &import.resolved_path else {
                continue;
            };
            for specifier in &import.specifiers {
                if let ImportSpecifier::Named { imported, local } = specifier {
                    edges.push((
                        importer.clone(),
                        import.source.clone(),
                        target.clone(),
                        imported.clone(),
                        local.clone(),
                    ));
                }
            }
        }
    }
    edges.sort();
    for (importer, source, target, name, local) in edges {
        if availability(ctx, Path::new(&target), &name, &mut HashSet::new()) == Availability::Absent
        {
            bail!(
                "The requested module '{}' does not provide an export named '{}' \
                 (imported as '{}' in {}). Resolved module: {}. \
                 For CommonJS properties not exposed as named exports, use a default import and read the property from it.",
                source, name, local, importer.display(), target
            );
        }
    }
    Ok(())
}
