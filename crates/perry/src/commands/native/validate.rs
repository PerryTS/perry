//! `perry native validate` — diff the manifest against what
//! actually got built.
//!
//! The flow:
//!   1. Parse `package.json` for the `perry.nativeLibrary` block.
//!   2. Run `cargo build --release` (skippable with `--no-build`).
//!   3. Locate `target/release/lib<crate>.a` (or `<crate>.lib` on
//!      Windows, or `lib<crate>.dylib`/`.so` for dylib crates).
//!   4. Run `nm -gU` (Unix) / `dumpbin /symbols` (Windows) and
//!      collect every defined symbol.
//!   5. Diff against the manifest's `functions[].name` array:
//!      - manifest function with no symbol → ERROR (broken binding)
//!      - symbol starting with `js_` not in manifest → WARNING
//!        (unreachable from TS — likely undeclared).
//!
//! Future work (open questions in #466):
//!   - Type-check `src/index.ts` against the manifest signatures
//!     (needs a JS-runtime dep or a vendored typecheck loop).
//!   - Validate per-target `frameworks` / `libs` actually exist
//!     on the host.

use anyhow::{anyhow, Context, Result};
use clap::Args;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::OutputFormat;

#[derive(Args, Debug)]
pub struct ValidateArgs {
    /// Path to the wrapper directory. Defaults to `.`.
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Skip the `cargo build --release` step. Useful when the
    /// staticlib is already built and you just want to re-check
    /// the symbol diff.
    #[arg(long)]
    pub no_build: bool,
}

pub fn run(args: ValidateArgs, format: OutputFormat, _use_color: bool) -> Result<()> {
    let pkg_path = args.path.join("package.json");
    let pkg_raw = std::fs::read_to_string(&pkg_path)
        .with_context(|| format!("reading {}", pkg_path.display()))?;
    let pkg: serde_json::Value = serde_json::from_str(&pkg_raw)
        .with_context(|| format!("parsing {}", pkg_path.display()))?;

    let manifest = pkg
        .pointer("/perry/nativeLibrary")
        .ok_or_else(|| anyhow!("no `perry.nativeLibrary` block in {}", pkg_path.display()))?;

    let declared_funcs: Vec<String> = manifest
        .pointer("/functions")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|f| f.get("name").and_then(|n| n.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let abi_version = manifest
        .pointer("/abiVersion")
        .and_then(|v| v.as_str())
        .map(String::from);

    let crate_name =
        read_crate_name(&args.path).context("reading [package].name from Cargo.toml")?;

    if !args.no_build {
        run_cargo_build(&args.path)?;
    }

    let staticlib = locate_staticlib(&args.path, &crate_name)?;
    let exported = read_exported_symbols(&staticlib)?;

    let declared_set: BTreeSet<&str> = declared_funcs.iter().map(|s| s.as_str()).collect();
    let exported_js_set: BTreeSet<&str> = exported
        .iter()
        .filter(|s| s.starts_with("js_"))
        .map(|s| s.as_str())
        .collect();

    let missing: Vec<&&str> = declared_set
        .iter()
        .filter(|f| !exported_js_set.contains(*f))
        .collect();
    let undeclared: Vec<&&str> = exported_js_set
        .iter()
        .filter(|s| !declared_set.contains(*s))
        .collect();

    match format {
        OutputFormat::Json => {
            let report = serde_json::json!({
                "package": pkg.get("name").cloned().unwrap_or(serde_json::Value::Null),
                "abiVersion": abi_version,
                "staticlib": staticlib.display().to_string(),
                "declared_functions": declared_funcs,
                "missing_symbols": missing,
                "undeclared_js_symbols": undeclared,
                "ok": missing.is_empty(),
            });
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        _ => {
            println!("perry native validate");
            println!("======================");
            println!(
                "  package:    {}",
                pkg.get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<unknown>")
            );
            println!(
                "  abiVersion: {}",
                abi_version
                    .as_deref()
                    .unwrap_or("<missing>  ⚠ required from v0.6.0")
            );
            println!("  staticlib:  {}", staticlib.display());
            println!();
            println!("  declared functions:           {}", declared_funcs.len());
            println!("  exported `js_*` symbols:      {}", exported_js_set.len());
            if !missing.is_empty() {
                println!();
                println!(
                    "  ❌ {} declared function(s) have NO matching symbol:",
                    missing.len()
                );
                for m in &missing {
                    println!("       {}", m);
                }
            }
            if !undeclared.is_empty() {
                println!();
                println!(
                    "  ⚠️  {} `js_*` symbol(s) NOT in the manifest \
                     (unreachable from user code):",
                    undeclared.len()
                );
                for u in &undeclared {
                    println!("       {}", u);
                }
            }
            if missing.is_empty() && undeclared.is_empty() {
                println!();
                println!("  ✅ manifest matches the staticlib.");
            }
        }
    }

    if !missing.is_empty() {
        std::process::exit(1);
    }
    Ok(())
}

fn read_crate_name(root: &Path) -> Result<String> {
    let cargo_toml = root.join("Cargo.toml");
    let raw = std::fs::read_to_string(&cargo_toml)
        .with_context(|| format!("reading {}", cargo_toml.display()))?;
    let parsed: toml::Value = toml::from_str(&raw)?;
    parsed
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .map(String::from)
        .ok_or_else(|| anyhow!("Cargo.toml has no `[package].name`"))
}

fn run_cargo_build(root: &Path) -> Result<()> {
    use std::process::Command;
    let status = Command::new("cargo")
        .arg("build")
        .arg("--release")
        .current_dir(root)
        .status()
        .context("invoking cargo build")?;
    if !status.success() {
        return Err(anyhow!("cargo build failed"));
    }
    Ok(())
}

fn locate_staticlib(root: &Path, crate_name: &str) -> Result<PathBuf> {
    let lib_basename = crate_name.replace('-', "_");
    // Refs #564: probe both `target/release/` and
    // `target/<host-triple>/release/`. Cargo writes to the
    // triple-prefixed dir whenever a default target is pinned via
    // `[build] target = "..."` (project / workspace / `~/.cargo/config.toml`)
    // or `CARGO_BUILD_TARGET` — common on Linux dev setups.
    let target_root = root.join("target");
    let mut search_dirs = vec![target_root.join("release")];
    if let Some(host) = crate::commands::compile::host_target_triple() {
        search_dirs.push(target_root.join(host).join("release"));
    }
    let lib_filenames = [
        format!("lib{}.a", lib_basename),
        format!("{}.lib", lib_basename),
        format!("lib{}.dylib", lib_basename),
        format!("lib{}.so", lib_basename),
    ];
    for dir in &search_dirs {
        for filename in &lib_filenames {
            let candidate = dir.join(filename);
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }
    Err(anyhow!(
        "no staticlib/dylib found in {:?} (looked for: {:?})",
        search_dirs
            .iter()
            .map(|d| d.display().to_string())
            .collect::<Vec<_>>(),
        lib_filenames
    ))
}

fn read_exported_symbols(staticlib: &Path) -> Result<Vec<String>> {
    use std::process::Command;
    // `nm -gP` works for .a, .dylib, .so on macOS + Linux. Windows
    // path uses dumpbin /symbols but we can fall back to letting
    // the Windows user pass a custom invocation in a follow-up; for
    // now we only support unix-style nm.
    let out = Command::new("nm")
        .arg("-gP")
        .arg(staticlib)
        .output()
        .with_context(|| format!("invoking nm on {}", staticlib.display()))?;
    if !out.status.success() {
        return Err(anyhow!(
            "nm failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut syms: Vec<String> = Vec::new();
    for line in stdout.lines() {
        // nm -gP format:
        //   _name T 0000000000000000 0000000000000000
        // We want only `T`/`R`/`D` (text/readonly/data — defined,
        // exported). Filter out `U` (undefined) entries.
        let mut cols = line.split_whitespace();
        let raw_name = match cols.next() {
            Some(n) => n,
            None => continue,
        };
        let kind = match cols.next() {
            Some(k) => k,
            None => continue,
        };
        if !matches!(kind, "T" | "R" | "D" | "S" | "B" | "C") {
            continue;
        }
        // macOS prepends `_`; Linux doesn't. Strip the leading `_`
        // so the name we compare against the manifest is uniform.
        let name = raw_name.strip_prefix('_').unwrap_or(raw_name);
        syms.push(name.to_string());
    }
    Ok(syms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_crate_name_works() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cargo = "[package]\nname = \"perry-ext-foo\"\nversion = \"0.1.0\"\n";
        std::fs::write(dir.path().join("Cargo.toml"), cargo).unwrap();
        let n = read_crate_name(dir.path()).expect("read");
        assert_eq!(n, "perry-ext-foo");
    }
}
