//! Small pure helpers split out of `collect_modules.rs`.
//!
//! Extracted only to keep that file under the 2000-line cap enforced by
//! `scripts/check_file_size.sh`; these three functions are self-contained
//! (no shared state with the collection walk) so the split is behaviour-free.

use std::collections::HashSet;
use std::path::Path;

fn object_string_property<'a>(
    object: &'a swc_ecma_ast::ObjectLit,
    name: &str,
) -> Option<&'a swc_ecma_ast::Str> {
    use swc_ecma_ast::{Expr, Lit, Prop, PropName, PropOrSpread};

    object.props.iter().find_map(|prop| {
        let PropOrSpread::Prop(prop) = prop else {
            return None;
        };
        let Prop::KeyValue(property) = prop.as_ref() else {
            return None;
        };
        let matches_name = match &property.key {
            PropName::Ident(key) => key.sym == *name,
            PropName::Str(key) => key.value == *name,
            _ => false,
        };
        if !matches_name {
            return None;
        }
        match property.value.as_ref() {
            Expr::Lit(Lit::Str(value)) => Some(value),
            _ => None,
        }
    })
}

fn object_property<'a>(
    object: &'a swc_ecma_ast::ObjectLit,
    name: &str,
) -> Option<&'a swc_ecma_ast::Expr> {
    use swc_ecma_ast::{Prop, PropName, PropOrSpread};

    object.props.iter().find_map(|prop| {
        let PropOrSpread::Prop(prop) = prop else {
            return None;
        };
        let Prop::KeyValue(property) = prop.as_ref() else {
            return None;
        };
        let matches_name = match &property.key {
            PropName::Ident(key) => key.sym == *name,
            PropName::Str(key) => key.value == *name,
            _ => false,
        };
        matches_name.then_some(property.value.as_ref())
    })
}

fn requests_asset_path(attributes: &swc_ecma_ast::ObjectLit) -> bool {
    object_string_property(attributes, "type")
        .and_then(|value| value.value.as_str())
        .is_some_and(|kind| matches!(kind, "file" | "wasm"))
}

/// Return imports that request an embedded asset path. This includes static
/// Bun file-loader imports and dynamic import options used by OpenCode:
///
/// `import path from "./asset.bin" with { type: "file" }`
/// `import("./tree-sitter.wasm", { with: { type: "wasm" } })`
pub(super) fn file_loader_import_sources(module: &swc_ecma_ast::Module) -> HashSet<String> {
    use swc_ecma_ast::{Callee, Expr, Lit, ModuleDecl, ModuleItem};
    use swc_ecma_visit::{Visit, VisitWith};

    let mut sources: HashSet<String> = module
        .body
        .iter()
        .filter_map(|item| {
            let ModuleItem::ModuleDecl(ModuleDecl::Import(import)) = item else {
                return None;
            };
            let attributes = import.with.as_deref()?;
            requests_asset_path(attributes)
                .then(|| import.src.value.as_str().map(str::to_owned))
                .flatten()
        })
        .collect();

    struct DynamicAssetImports<'a>(&'a mut HashSet<String>);
    impl Visit for DynamicAssetImports<'_> {
        fn visit_call_expr(&mut self, call: &swc_ecma_ast::CallExpr) {
            if matches!(call.callee, Callee::Import(_)) {
                let source = call.args.first().and_then(|arg| match arg.expr.as_ref() {
                    Expr::Lit(Lit::Str(source)) => source.value.as_str(),
                    _ => None,
                });
                let options = call.args.get(1).and_then(|arg| match arg.expr.as_ref() {
                    Expr::Object(options) => Some(options),
                    _ => None,
                });
                let attributes = options.and_then(|options| {
                    object_property(options, "with")
                        .or_else(|| object_property(options, "assert"))
                        .and_then(|value| match value {
                            Expr::Object(attributes) => Some(attributes),
                            _ => None,
                        })
                        .or(Some(options))
                });
                if let (Some(source), Some(attributes)) = (source, attributes) {
                    if requests_asset_path(attributes) {
                        self.0.insert(source.to_string());
                    }
                }
            }
            call.visit_children_with(self);
        }
    }
    module.visit_with(&mut DynamicAssetImports(&mut sources));
    sources
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
