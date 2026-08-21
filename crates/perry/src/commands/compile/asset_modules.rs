//! Deterministic virtual modules for directories of packaged files.
//!
//! Bun's build API lets applications inject a generated module whose default
//! export maps logical file names to `type: "file"` imports. Source-first AOT
//! compilation has no bundler injection phase, so `--asset-module` reproduces
//! that narrow operation before module collection and stores the generated
//! source in Perry's cache.

use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use sha2::{Digest, Sha256};

use super::{CompilationContext, GeneratedAssetModule};
use crate::OutputFormat;

pub(super) fn generate(
    specs: &[String],
    ctx: &mut CompilationContext,
    format: OutputFormat,
) -> Result<()> {
    for spec in specs {
        let (module_specifier, directory) = spec.split_once('=').ok_or_else(|| {
            anyhow!(
                "invalid --asset-module `{spec}`; expected <module-specifier>=<asset-directory>"
            )
        })?;
        let module_specifier = module_specifier.trim();
        let directory = directory.trim();
        if module_specifier.is_empty() || directory.is_empty() {
            bail!(
                "invalid --asset-module `{spec}`; both the module specifier and asset directory are required"
            );
        }
        if Path::new(module_specifier)
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        {
            bail!(
                "invalid --asset-module specifier `{module_specifier}`; use a bare or nested module name without `.` or `..` path components"
            );
        }
        if ctx.generated_asset_modules.contains_key(module_specifier) {
            bail!("duplicate --asset-module specifier `{module_specifier}`");
        }

        let requested_root = Path::new(directory);
        let requested_root = if requested_root.is_absolute() {
            requested_root.to_path_buf()
        } else {
            ctx.cache_root.join(requested_root)
        };
        let asset_root = requested_root.canonicalize().with_context(|| {
            format!(
                "asset directory for generated module `{module_specifier}` was not found: {}\n  \
                 Run the upstream asset build/preparation command first, then retry Perry compile.",
                requested_root.display()
            )
        })?;
        if !asset_root.is_dir() {
            bail!(
                "asset-module source for `{module_specifier}` is not a directory: {}",
                asset_root.display()
            );
        }

        let mut assets: Vec<(String, PathBuf)> = walkdir::WalkDir::new(&asset_root)
            .follow_links(false)
            .into_iter()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_file())
            .filter_map(|entry| {
                let path = entry.into_path();
                let relative = path.strip_prefix(&asset_root).ok()?;
                let logical = slash_path(relative);
                (!logical.ends_with(".map")).then_some((logical, path))
            })
            .collect();
        assets.sort_by(|a, b| a.0.cmp(&b.0));
        if assets.is_empty() {
            bail!(
                "asset directory for generated module `{module_specifier}` contains no packageable files: {}\n  \
                 Run the upstream asset build/preparation command first, then retry Perry compile.",
                asset_root.display()
            );
        }

        let generated_dir = ctx.cache_dir.join("generated-asset-modules");
        fs::create_dir_all(&generated_dir).with_context(|| {
            format!(
                "failed to create generated asset-module cache directory {}",
                generated_dir.display()
            )
        })?;
        let module_hash = short_hash(module_specifier);
        let generated_path = generated_dir.join(format!("{module_hash}.gen.ts"));
        let logical_root = ctx
            .cache_root
            .canonicalize()
            .unwrap_or_else(|_| ctx.cache_root.clone());
        let logical_path = logical_root.join(module_specifier);
        let logical_dir = logical_path.parent().unwrap_or(&ctx.cache_root);

        let mut source =
            String::from("// Generated deterministically by Perry --asset-module. Do not edit.\n");
        for (index, (logical, path)) in assets.iter().enumerate() {
            let canonical = path
                .canonicalize()
                .with_context(|| format!("failed to resolve packaged asset {}", path.display()))?;
            let import_path = relative_path(logical_dir, &canonical).ok_or_else(|| {
                anyhow!(
                    "cannot express asset {} relative to generated module `{module_specifier}`",
                    canonical.display()
                )
            })?;
            let mut import_path = slash_path(&import_path);
            if !import_path.starts_with('.') {
                import_path.insert_str(0, "./");
            }
            source.push_str(&format!(
                "import file_{index} from {} with {{ type: \"file\" }};\n",
                serde_json::to_string(&import_path)?
            ));
            let identity = format!("{module_specifier}\0{logical}");
            let packaged_name = format!(
                "__perry_imports/{}/{filename}",
                short_hash(&identity),
                filename = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("asset.bin")
            );
            ctx.file_loader_asset_names.insert(canonical, packaged_name);
        }
        source.push_str("export default {\n");
        for (index, (logical, _)) in assets.iter().enumerate() {
            source.push_str(&format!(
                "  {}: file_{index},\n",
                serde_json::to_string(logical)?
            ));
        }
        source.push_str("};\n");
        fs::write(&generated_path, source).with_context(|| {
            format!(
                "failed to write generated asset module {}",
                generated_path.display()
            )
        })?;
        let source_path = generated_path.canonicalize().with_context(|| {
            format!(
                "failed to resolve generated asset module {}",
                generated_path.display()
            )
        })?;

        if matches!(format, OutputFormat::Text) {
            println!(
                "  Asset module: {module_specifier} ({} files from {})",
                assets.len(),
                asset_root.display()
            );
        }
        ctx.generated_asset_modules.insert(
            module_specifier.to_string(),
            GeneratedAssetModule {
                source_path,
                logical_path,
                asset_root,
                assets,
            },
        );
    }
    Ok(())
}

fn slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
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

fn short_hash(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    use crate::commands::progress::VerboseProgress;

    #[test]
    fn generation_is_sorted_excludes_sourcemaps_and_uses_stable_handles() {
        let project = tempfile::tempdir().unwrap();
        let assets = project.path().join("dist");
        fs::create_dir_all(assets.join("nested")).unwrap();
        fs::write(assets.join("z.js"), "z").unwrap();
        fs::write(assets.join("nested/a.css"), "a").unwrap();
        fs::write(assets.join("z.js.map"), "map").unwrap();

        let mut ctx = CompilationContext::new(project.path().to_path_buf());
        ctx.cache_root = project.path().to_path_buf();
        ctx.cache_dir = project.path().join("cache");
        generate(
            &["web-ui.gen.ts=dist".to_string()],
            &mut ctx,
            OutputFormat::Json,
        )
        .unwrap();

        let generated = &ctx.generated_asset_modules["web-ui.gen.ts"];
        assert_eq!(
            generated
                .assets
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            ["nested/a.css", "z.js"]
        );
        let source = fs::read_to_string(&generated.source_path).unwrap();
        assert!(source.find("nested/a.css").unwrap() < source.find("z.js").unwrap());
        assert!(!source.contains("z.js.map"));
        assert!(!source.contains(project.path().to_string_lossy().as_ref()));

        let first_names = ctx.file_loader_asset_names.clone();
        let mut second = CompilationContext::new(project.path().to_path_buf());
        second.cache_root = project.path().to_path_buf();
        second.cache_dir = project.path().join("cache-2");
        generate(
            &["web-ui.gen.ts=dist".to_string()],
            &mut second,
            OutputFormat::Json,
        )
        .unwrap();
        assert_eq!(first_names, second.file_loader_asset_names);

        let other_project = tempfile::tempdir().unwrap();
        let other_assets = other_project.path().join("dist");
        fs::create_dir_all(other_assets.join("nested")).unwrap();
        fs::write(other_assets.join("z.js"), "z").unwrap();
        fs::write(other_assets.join("nested/a.css"), "a").unwrap();
        fs::write(other_assets.join("z.js.map"), "map").unwrap();
        let mut relocated = CompilationContext::new(other_project.path().to_path_buf());
        relocated.cache_root = other_project.path().to_path_buf();
        relocated.cache_dir = other_project.path().join("cache");
        generate(
            &["web-ui.gen.ts=dist".to_string()],
            &mut relocated,
            OutputFormat::Json,
        )
        .unwrap();
        let relocated_source =
            fs::read_to_string(&relocated.generated_asset_modules["web-ui.gen.ts"].source_path)
                .unwrap();
        assert_eq!(source, relocated_source);
        let mut first_handles: Vec<_> = first_names.into_values().collect();
        let mut relocated_handles: Vec<_> =
            relocated.file_loader_asset_names.into_values().collect();
        first_handles.sort();
        relocated_handles.sort();
        assert_eq!(first_handles, relocated_handles);
    }

    #[test]
    fn missing_directory_has_preparation_remediation() {
        let project = tempfile::tempdir().unwrap();
        let mut ctx = CompilationContext::new(project.path().to_path_buf());
        ctx.cache_root = project.path().to_path_buf();
        let error = generate(
            &["web-ui.gen.ts=missing-dist".to_string()],
            &mut ctx,
            OutputFormat::Json,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("was not found"));
        assert!(error.contains("upstream asset build/preparation command"));
    }

    #[test]
    fn generated_bare_module_collects_file_loader_assets_and_writes_manifest() {
        let project = tempfile::tempdir().unwrap();
        let assets = project.path().join("dist");
        fs::create_dir_all(&assets).unwrap();
        fs::write(assets.join("index.html"), "<h1>Perry</h1>").unwrap();
        let entry = project.path().join("entry.ts");
        fs::write(
            &entry,
            r#"
export async function ui() {
  return import("opencode-web-ui.gen.ts");
}
"#,
        )
        .unwrap();

        let mut ctx = CompilationContext::new(project.path().to_path_buf());
        ctx.cache_root = project.path().to_path_buf();
        ctx.cache_dir = project.path().join("cache");
        ctx.entry_canonical = Some(entry.canonicalize().unwrap());
        generate(
            &["opencode-web-ui.gen.ts=dist".to_string()],
            &mut ctx,
            OutputFormat::Json,
        )
        .unwrap();

        let mut visited = HashSet::new();
        let mut next_class_id: perry_hir::ClassId = 1;
        let progress = VerboseProgress::new(OutputFormat::Json, 0);
        super::super::collect_modules(
            &entry,
            &mut ctx,
            &mut visited,
            OutputFormat::Json,
            None,
            &mut next_class_id,
            false,
            &progress,
            None,
        )
        .unwrap();

        assert_eq!(ctx.file_loader_asset_paths.len(), 1);
        assert_eq!(ctx.embedded_assets.len(), 1);
        let html = assets.join("index.html").canonicalize().unwrap();
        let html_hir = format!("{:?}", ctx.native_modules.get(&html).unwrap());
        assert!(html_hir.contains("$perryfs/__perry_imports/"));
        assert!(!html_hir.contains("<h1>Perry</h1>"));
        let embedded = ctx.embedded_assets.clone();
        super::super::asset_manifest::write(&ctx, &embedded, OutputFormat::Json).unwrap();
        let manifest = fs::read_to_string(ctx.cache_dir.join("assets.json")).unwrap();
        assert!(manifest.contains("opencode-web-ui.gen.ts"));
        assert!(manifest.contains("index.html"));
        assert!(manifest.contains("$perryfs/__perry_imports/"));
        assert!(!manifest.contains(project.path().to_string_lossy().as_ref()));
    }
}
