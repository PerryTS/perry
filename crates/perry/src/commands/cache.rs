//! Cache management subcommands: `perry cache clean`, `perry cache info`.
//!
//! The on-disk object cache lives at `<project-root>/.perry-cache/objects/<target>/<key>.o`
//! (see `commands/compile.rs :: ObjectCache`). These subcommands let users
//! inspect and wipe it without resorting to `rm -rf`, which matters mostly for
//! discoverability: if a user suspects a stale cache they can reach for
//! `perry cache clean` rather than digging into a hidden directory.

use anyhow::{anyhow, Result};
use clap::{Args, Subcommand};
use std::fs;
use std::path::{Path, PathBuf};

use crate::OutputFormat;

#[derive(Args, Debug)]
pub struct CacheArgs {
    #[command(subcommand)]
    pub command: CacheCommand,
}

#[derive(Subcommand, Debug)]
pub enum CacheCommand {
    /// Delete the entire `.perry-cache/` directory for the current project.
    Clean(CleanArgs),
    /// Show cache location, size, and per-target file counts.
    Info(InfoArgs),
}

#[derive(Args, Debug)]
pub struct CleanArgs {
    /// Project root (defaults to the current working directory).
    #[arg(long)]
    pub project_root: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct InfoArgs {
    /// Project root (defaults to the current working directory).
    #[arg(long)]
    pub project_root: Option<PathBuf>,
}

pub fn run(args: CacheArgs, format: OutputFormat) -> Result<()> {
    match args.command {
        CacheCommand::Clean(a) => clean(a, format),
        CacheCommand::Info(a) => info(a, format),
    }
}

fn resolve_root(explicit: Option<PathBuf>) -> Result<PathBuf> {
    match explicit {
        Some(p) => Ok(p),
        None => std::env::current_dir()
            .map_err(|e| anyhow!("failed to determine current directory: {}", e)),
    }
}

fn clean(args: CleanArgs, format: OutputFormat) -> Result<()> {
    let root = resolve_root(args.project_root)?;
    let cache_dir = root.join(".perry-cache");
    if !cache_dir.exists() {
        match format {
            OutputFormat::Text => {
                println!("No cache found at {}", cache_dir.display());
            }
            OutputFormat::Json => {
                println!(
                    "{{\"removed\":false,\"path\":{}}}",
                    serde_json::to_string(&cache_dir.display().to_string())?
                );
            }
        }
        return Ok(());
    }
    let (files, bytes) = measure_dir(&cache_dir);
    fs::remove_dir_all(&cache_dir)
        .map_err(|e| anyhow!("failed to remove {}: {}", cache_dir.display(), e))?;
    match format {
        OutputFormat::Text => {
            println!(
                "Removed {} ({} file{}, {:.1} MB)",
                cache_dir.display(),
                files,
                if files == 1 { "" } else { "s" },
                bytes as f64 / (1024.0 * 1024.0)
            );
        }
        OutputFormat::Json => {
            println!(
                "{{\"removed\":true,\"path\":{},\"files\":{},\"bytes\":{}}}",
                serde_json::to_string(&cache_dir.display().to_string())?,
                files,
                bytes
            );
        }
    }
    Ok(())
}

fn info(args: InfoArgs, format: OutputFormat) -> Result<()> {
    let root = resolve_root(args.project_root)?;
    let cache_dir = root.join(".perry-cache");
    if !cache_dir.exists() {
        match format {
            OutputFormat::Text => {
                println!("No cache at {}", cache_dir.display());
            }
            OutputFormat::Json => {
                println!(
                    "{{\"exists\":false,\"path\":{}}}",
                    serde_json::to_string(&cache_dir.display().to_string())?
                );
            }
        }
        return Ok(());
    }
    let objects_dir = cache_dir.join("objects");
    let mut per_target: Vec<(String, usize, u64)> = Vec::new();
    if objects_dir.exists() {
        if let Ok(rd) = fs::read_dir(&objects_dir) {
            for entry in rd.flatten() {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let (files, bytes) = measure_dir(&entry.path());
                    per_target.push((name, files, bytes));
                }
            }
        }
    }
    per_target.sort_by(|a, b| a.0.cmp(&b.0));
    let (total_files, total_bytes) = measure_dir(&cache_dir);
    match format {
        OutputFormat::Text => {
            println!("Cache: {}", cache_dir.display());
            println!(
                "Total: {} file{}, {:.1} MB",
                total_files,
                if total_files == 1 { "" } else { "s" },
                total_bytes as f64 / (1024.0 * 1024.0)
            );
            if per_target.is_empty() {
                println!("  (no object cache entries)");
            } else {
                for (tgt, files, bytes) in &per_target {
                    println!(
                        "  objects/{}: {} entr{}, {:.1} MB",
                        tgt,
                        files,
                        if *files == 1 { "y" } else { "ies" },
                        *bytes as f64 / (1024.0 * 1024.0)
                    );
                }
            }
        }
        OutputFormat::Json => {
            let targets_json = per_target
                .iter()
                .map(|(t, f, b)| {
                    format!(
                        "{{\"target\":{},\"files\":{},\"bytes\":{}}}",
                        serde_json::to_string(t).unwrap_or_else(|_| "\"?\"".to_string()),
                        f,
                        b
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            println!(
                "{{\"exists\":true,\"path\":{},\"total_files\":{},\"total_bytes\":{},\"targets\":[{}]}}",
                serde_json::to_string(&cache_dir.display().to_string())?,
                total_files,
                total_bytes,
                targets_json
            );
        }
    }
    Ok(())
}

/// Recursively count regular files and sum their sizes under `path`.
/// IO errors are silently skipped — the result is informational only.
fn measure_dir(path: &Path) -> (usize, u64) {
    let mut files = 0usize;
    let mut bytes = 0u64;
    if let Ok(rd) = fs::read_dir(path) {
        for entry in rd.flatten() {
            let ep = entry.path();
            match entry.file_type() {
                Ok(ft) if ft.is_dir() => {
                    let (f, b) = measure_dir(&ep);
                    files += f;
                    bytes += b;
                }
                Ok(ft) if ft.is_file() => {
                    files += 1;
                    if let Ok(meta) = entry.metadata() {
                        bytes += meta.len();
                    }
                }
                _ => {}
            }
        }
    }
    (files, bytes)
}
