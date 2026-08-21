//! Deterministic provenance report for source-graph assets.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{is_recognized_text_asset, CompilationContext};
use crate::OutputFormat;

#[derive(Serialize)]
struct AssetManifest {
    version: u32,
    generated_modules: Vec<GeneratedModuleRecord>,
    assets: Vec<AssetRecord>,
}

#[derive(Serialize)]
struct GeneratedModuleRecord {
    specifier: String,
    source_directory: String,
    packaged_files: usize,
}

#[derive(Clone, Serialize)]
struct AssetRecord {
    kind: &'static str,
    source: String,
    packaged_path: String,
    size: u64,
    sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    generated_module: Option<String>,
}

pub(super) fn write(
    ctx: &CompilationContext,
    embedded_assets: &[(String, PathBuf)],
    format: OutputFormat,
) -> std::io::Result<()> {
    let mut records: BTreeMap<(String, String), AssetRecord> = BTreeMap::new();

    for path in ctx.native_modules.keys() {
        if ctx.file_loader_asset_paths.contains(path) {
            continue;
        }
        let kind = if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            Some("json")
        } else if is_recognized_text_asset(path) {
            Some("text")
        } else {
            None
        };
        if let Some(kind) = kind {
            insert_record(
                &mut records,
                record_for(
                    ctx,
                    path,
                    kind,
                    format!("module:{}", source_origin(path, &ctx.cache_root)),
                    None,
                )?,
            );
        }
    }

    for (packaged_name, path) in embedded_assets {
        let kind = if path.extension().and_then(|ext| ext.to_str()) == Some("wasm") {
            "wasm"
        } else if ctx.file_loader_asset_paths.contains(path) {
            "file"
        } else {
            "embedded"
        };
        insert_record(
            &mut records,
            record_for(
                ctx,
                path,
                kind,
                format!("$perryfs/{packaged_name}"),
                generated_owner(ctx, path),
            )?,
        );
    }

    let generated_modules = ctx
        .generated_asset_modules
        .iter()
        .map(|(specifier, generated)| GeneratedModuleRecord {
            specifier: specifier.clone(),
            source_directory: source_origin(&generated.asset_root, &ctx.cache_root),
            packaged_files: generated.assets.len(),
        })
        .collect();
    let manifest = AssetManifest {
        version: 1,
        generated_modules,
        assets: records.into_values().collect(),
    };
    fs::create_dir_all(&ctx.cache_dir)?;
    let path = ctx.cache_dir.join("assets.json");
    let mut json = serde_json::to_string_pretty(&manifest).map_err(std::io::Error::other)?;
    json.push('\n');
    fs::write(&path, json)?;
    if matches!(format, OutputFormat::Text) {
        println!("Asset manifest: {}", path.display());
    }
    Ok(())
}

fn insert_record(records: &mut BTreeMap<(String, String), AssetRecord>, record: AssetRecord) {
    records.insert(
        (record.packaged_path.clone(), record.source.clone()),
        record,
    );
}

fn record_for(
    ctx: &CompilationContext,
    path: &Path,
    kind: &'static str,
    packaged_path: String,
    generated_module: Option<String>,
) -> std::io::Result<AssetRecord> {
    let bytes = fs::read(path)?;
    Ok(AssetRecord {
        kind,
        source: source_origin(path, &ctx.cache_root),
        packaged_path,
        size: bytes.len() as u64,
        sha256: Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        generated_module,
    })
}

fn generated_owner(ctx: &CompilationContext, path: &Path) -> Option<String> {
    ctx.generated_asset_modules
        .iter()
        .find_map(|(specifier, generated)| {
            path.starts_with(&generated.asset_root)
                .then(|| specifier.clone())
        })
}

/// Return a slash-normalized path relative to the project/package root,
/// retaining `..` components for monorepo sibling assets.
fn source_origin(path: &Path, root: &Path) -> String {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    relative_path(&root, &path)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn relative_path(from: &Path, to: &Path) -> Option<PathBuf> {
    let from: Vec<Component<'_>> = from.components().collect();
    let to: Vec<Component<'_>> = to.components().collect();
    if from.first() != to.first() {
        return None;
    }
    let common = from
        .iter()
        .zip(&to)
        .take_while(|(left, right)| left == right)
        .count();
    let mut result = PathBuf::new();
    for _ in common..from.len() {
        result.push("..");
    }
    for component in &to[common..] {
        result.push(component.as_os_str());
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_origins_are_checkout_independent() {
        assert_eq!(
            relative_path(
                Path::new("/checkout/pkg"),
                Path::new("/checkout/app/dist/a.js")
            ),
            Some(PathBuf::from("../app/dist/a.js"))
        );
    }
}
