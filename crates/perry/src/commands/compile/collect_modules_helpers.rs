//! Small pure helpers split out of `collect_modules.rs`.
//!
//! Extracted only to keep that file under the 2000-line cap enforced by
//! `scripts/check_file_size.sh`; these three functions are self-contained
//! (no shared state with the collection walk) so the split is behaviour-free.

use std::collections::HashSet;
use std::path::Path;

/// Return imports that request Bun's file loader:
/// `import path from "./asset.bin" with { type: "file" }`.
pub(super) fn file_loader_import_sources(module: &swc_ecma_ast::Module) -> HashSet<String> {
    use swc_ecma_ast::{Expr, Lit, ModuleDecl, ModuleItem, Prop, PropName, PropOrSpread};

    module
        .body
        .iter()
        .filter_map(|item| {
            let ModuleItem::ModuleDecl(ModuleDecl::Import(import)) = item else {
                return None;
            };
            let attributes = import.with.as_deref()?;
            let uses_file_loader = attributes.props.iter().any(|prop| {
                let PropOrSpread::Prop(prop) = prop else {
                    return false;
                };
                let Prop::KeyValue(property) = prop.as_ref() else {
                    return false;
                };
                let is_type = match &property.key {
                    PropName::Ident(name) => name.sym == *"type",
                    PropName::Str(name) => name.value == *"type",
                    _ => false,
                };
                is_type
                    && matches!(
                        property.value.as_ref(),
                        Expr::Lit(Lit::Str(value)) if value.value == *"file"
                    )
            });
            uses_file_loader.then(|| import.src.value.as_str().unwrap_or("").to_string())
        })
        .collect()
}

/// Produce a stable virtual asset name without leaking an absolute source path.
pub(super) fn imported_file_asset_name(path: &Path, project_root: &Path) -> String {
    // Hash the source identity relative to the package root whenever possible.
    // Hashing the canonical absolute path made otherwise identical builds in
    // two checkout directories expose different `$perryfs` handles.
    let identity = path.strip_prefix(project_root).unwrap_or(path);
    let normalized = identity.to_string_lossy().replace('\\', "/");
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in normalized.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("asset.bin");
    format!("__perry_imports/{hash:016x}/{filename}")
}

pub(super) fn looks_like_generated_module(specifier: &str) -> bool {
    let filename = specifier.rsplit('/').next().unwrap_or(specifier);
    filename.contains(".gen.")
        || filename.contains(".generated.")
        || filename.starts_with("generated-")
}
