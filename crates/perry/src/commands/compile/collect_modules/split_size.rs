//! Estimate namespace expansion by walking the already collected HIR graph.
use super::*;

fn resolved<'a>(ctx: &'a CompilationContext, from: &Path, source: &str) -> Option<&'a PathBuf> {
    if let Some(module) = ctx.native_modules.get(from) {
        if let Some(path) = module.imports.iter().find_map(|i| {
            (i.source == source)
                .then_some(i.resolved_path.as_deref())
                .flatten()
        }) {
            if let Some((key, _)) = ctx.native_modules.get_key_value(Path::new(path)) {
                return Some(key);
            }
        }
    }
    let parent = from.parent()?;
    if let Some(Some((path, _))) = ctx
        .resolve_cache
        .get(&(source.to_owned(), parent.to_owned()))
    {
        return Some(path);
    }
    // Collection resolves against the lexical importer first. Reuse that
    // existing graph edge when the worktree/fixture path is a symlink.
    ctx.resolve_cache
        .iter()
        .find_map(|((specifier, base), result)| {
            let base = if base.as_os_str().is_empty() {
                Path::new(".")
            } else {
                base.as_path()
            };
            if specifier != source || base.canonicalize().ok().as_deref() != Some(parent) {
                return None;
            }
            result.as_ref().map(|(path, _)| path)
        })
}

fn type_only(module: &perry_hir::Module, name: &str) -> bool {
    let is_type = module.interfaces.iter().any(|t| t.name == name)
        || module.type_aliases.iter().any(|t| t.name == name);
    is_type
        && !(module.functions.iter().any(|f| f.name == name)
            || module.exported_functions.iter().any(|(n, _)| n == name)
            || module.exported_objects.iter().any(|n| n == name)
            || module.classes.iter().any(|c| c.name == name)
            || module.enums.iter().any(|e| e.name == name)
            || module.globals.iter().any(|g| g.name == name))
}

fn exports(
    ctx: &CompilationContext,
    path: &PathBuf,
    seen: &mut Vec<PathBuf>,
    include_default: bool,
) -> usize {
    if seen.contains(path) {
        return 0;
    }
    let Some(module) = ctx.native_modules.get(path) else {
        return 0;
    };
    seen.push(path.clone());
    let mut n = 0;
    for e in &module.exports {
        match e {
            perry_hir::Export::Named { local, exported } => {
                n += usize::from(
                    (include_default || exported != "default") && !type_only(module, local),
                );
            }
            perry_hir::Export::ReExport { exported, .. } => {
                n += usize::from(include_default || exported != "default");
            }
            perry_hir::Export::NamespaceReExport { .. } => n += 1,
            perry_hir::Export::ExportAll { source } => {
                if let Some(p) = resolved(ctx, path, source) {
                    n += exports(ctx, p, seen, false);
                }
            }
        }
    }
    // The per-query walk visits each module once, stopping both cycles and
    // shared star subgraphs. No export-size map survives the walk.
    n
}

pub(super) fn namespace_members(ctx: &CompilationContext, from: &Path, source: &str) -> usize {
    let path = if Path::new(source).is_absolute() {
        ctx.native_modules
            .get_key_value(Path::new(source))
            .map(|(key, _)| key)
    } else {
        resolved(ctx, from, source)
    };
    path.map(|path| exports(ctx, path, &mut Vec::new(), true))
        .unwrap_or(0)
}
