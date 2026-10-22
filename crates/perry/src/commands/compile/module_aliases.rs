//! `--module-alias <specifier|module-file>=<file>`: resolve a module to a
//! source file. Bun builds map virtual modules (for example
//! `virtual:opencode-app-assets`) to generated source with a plugin's
//! onResolve hook, and replace a module's source with its onLoad hook (for
//! example OpenCode's generated `plugin/src/runtime-modules.ts`).
//! Source-first AOT compilation has no plugin phase, so the build hands Perry
//! the generated file directly. A key written as a path (`./`, `../`, `/`)
//! names a module file; any other key is a module specifier.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};

use super::CompilationContext;

pub(super) fn register(specs: &[String], ctx: &mut CompilationContext) -> Result<()> {
    for spec in specs {
        let (key, file) = spec.split_once('=').ok_or_else(|| {
            anyhow!(
                "invalid --module-alias `{spec}`; expected <module-specifier|module-file>=<file>"
            )
        })?;
        let (key, file) = (key.trim(), file.trim());
        if key.is_empty() || file.is_empty() {
            bail!("invalid --module-alias `{spec}`; both the module and the file are required");
        }
        let target = existing_file(ctx, file, key, "file")?;
        if is_path_key(key) {
            let original = existing_file(ctx, key, key, "module file")?;
            if ctx.module_file_aliases.insert(original, target).is_some() {
                bail!("duplicate module file `{key}` in --module-alias");
            }
        } else {
            if ctx.module_aliases.contains_key(key) || ctx.generated_asset_modules.contains_key(key)
            {
                bail!("duplicate module specifier `{key}` in --module-alias");
            }
            ctx.module_aliases.insert(key.to_string(), target);
        }
    }
    Ok(())
}

fn is_path_key(key: &str) -> bool {
    key.starts_with("./") || key.starts_with("../") || Path::new(key).is_absolute()
}

fn existing_file(ctx: &CompilationContext, file: &str, key: &str, what: &str) -> Result<PathBuf> {
    let requested = Path::new(file);
    let requested = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        ctx.cache_root.join(requested)
    };
    let canonical = requested.canonicalize().with_context(|| {
        format!(
            "--module-alias {what} for `{key}` was not found: {}",
            requested.display()
        )
    })?;
    if !canonical.is_file() {
        bail!(
            "--module-alias {what} for `{key}` is not a file: {}",
            canonical.display()
        );
    }
    Ok(canonical)
}
