//! `perry lock` subcommand — #498.
//!
//! User-facing CLI for the lockfile that `perry compile` writes /
//! verifies as a side effect. Three flows:
//!
//! - **`perry lock`** — verify-and-write the lockfile (default mode).
//! - **`perry lock --update <pkg>...`** — refresh the named
//!   package(s) after a deliberate upgrade.
//! - **`perry lock --frozen`** — CI gate. Refuses to write the
//!   lockfile; missing AND mismatched entries both fail.

use anyhow::Result;
use clap::Args;
use std::path::PathBuf;

use crate::commands::perry_lock::LockMode;

#[derive(Args, Debug)]
pub struct LockArgs {
    /// Project root containing `package.json` (and where `perry.lock`
    /// will be created / verified). Defaults to the current
    /// directory.
    #[arg(long, default_value = ".")]
    pub project_root: PathBuf,

    /// Entry file (.ts). Reserved for a future extension that walks
    /// the import graph instead of `node_modules/*`; currently the
    /// lock subcommand discovers archives by scanning every
    /// `perry.nativeLibrary`-declaring package directly under
    /// `node_modules`.
    #[arg(long)]
    pub input: Option<PathBuf>,

    /// Compilation target - must match what the build uses, since
    /// the archive set is target-dependent (per-arch prebuilts).
    /// Defaults to native host target.
    #[arg(long)]
    pub target: Option<String>,

    /// `--update <pkg>` - refresh the SHA-256 hash for `<pkg>` in
    /// `perry.lock` even if it currently mismatches. Repeat to
    /// refresh multiple packages. Run after a deliberate dep
    /// upgrade.
    #[arg(long, value_name = "PKG")]
    pub update: Vec<String>,

    /// `--update-all` - opt every mismatched package into the
    /// refresh. Equivalent to `--update` for every package at once.
    #[arg(long)]
    pub update_all: bool,

    /// `--frozen` - verification-only mode for CI. Refuses to write
    /// `perry.lock`. Missing AND mismatched entries both fail.
    #[arg(long, conflicts_with = "update", conflicts_with = "update_all")]
    pub frozen: bool,
}

impl LockArgs {
    /// Materialize the [`LockMode`] this invocation requests.
    pub fn mode(&self) -> LockMode {
        if self.frozen {
            LockMode::Frozen
        } else if self.update_all || !self.update.is_empty() {
            let pkgs = if self.update_all {
                Vec::new()
            } else {
                self.update.clone()
            };
            LockMode::Update(pkgs)
        } else {
            LockMode::Default
        }
    }
}

/// Entry point for `perry lock`. Discovers every prebuilt archive
/// the project would consume at link time and runs the lock
/// verify/write pass in the requested mode.
pub fn run(args: LockArgs, format: crate::OutputFormat, _use_color: bool) -> Result<()> {
    let mode = args.mode();

    let archives = crate::commands::compile::collect_native_archives_for_lock(
        &args.project_root,
        args.input.as_deref(),
        args.target.as_deref(),
        format,
    )?;

    // Canonicalize project root so the lockfile lives next to
    // package.json regardless of how `--project-root` was spelled.
    let project_root_canonical = args
        .project_root
        .canonicalize()
        .unwrap_or(args.project_root.clone());
    let lock =
        crate::commands::perry_lock::verify_or_write(&project_root_canonical, &archives, &mode)?;

    match format {
        crate::OutputFormat::Text => match &mode {
            LockMode::Frozen => {
                println!(
                    "perry.lock verified ({} package(s), {} archive(s)).",
                    lock.native_library.len(),
                    archives.len()
                );
            }
            LockMode::Update(pkgs) => {
                if pkgs.is_empty() {
                    println!("perry.lock refreshed ({} archive(s)).", archives.len());
                } else {
                    println!("perry.lock refreshed for: {}", pkgs.join(", "));
                }
            }
            LockMode::Default => {
                println!(
                    "perry.lock up to date ({} package(s), {} archive(s)).",
                    lock.native_library.len(),
                    archives.len()
                );
            }
        },
        crate::OutputFormat::Json => {
            println!(
                "{}",
                serde_json::json!({
                    "mode": match &mode {
                        LockMode::Default => "default",
                        LockMode::Update(_) => "update",
                        LockMode::Frozen => "frozen",
                    },
                    "package_count": lock.native_library.len(),
                    "archive_count": archives.len(),
                })
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_prefers_frozen_when_set() {
        let args = LockArgs {
            project_root: ".".into(),
            input: None,
            target: None,
            update: vec![],
            update_all: false,
            frozen: true,
        };
        assert!(matches!(args.mode(), LockMode::Frozen));
    }

    #[test]
    fn mode_update_with_packages() {
        let args = LockArgs {
            project_root: ".".into(),
            input: None,
            target: None,
            update: vec!["pkg-a".into(), "pkg-b".into()],
            update_all: false,
            frozen: false,
        };
        match args.mode() {
            LockMode::Update(p) => assert_eq!(p, vec!["pkg-a".to_string(), "pkg-b".to_string()]),
            _ => panic!("expected Update"),
        }
    }

    #[test]
    fn mode_update_all_no_packages() {
        let args = LockArgs {
            project_root: ".".into(),
            input: None,
            target: None,
            update: vec![],
            update_all: true,
            frozen: false,
        };
        match args.mode() {
            LockMode::Update(p) => assert!(p.is_empty()),
            _ => panic!("expected Update"),
        }
    }

    #[test]
    fn mode_default_when_nothing_set() {
        let args = LockArgs {
            project_root: ".".into(),
            input: None,
            target: None,
            update: vec![],
            update_all: false,
            frozen: false,
        };
        assert!(matches!(args.mode(), LockMode::Default));
    }
}
