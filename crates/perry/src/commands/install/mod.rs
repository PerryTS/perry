//! `perry install` — secure wrapper around bun/npm.
//!
//! Architecture: shell out to `bun install --ignore-scripts` (or `npm install
//! --ignore-scripts` as fallback) to get a populated `node_modules/` with
//! *no* lifecycle scripts run, then statically scan the tree for known
//! malware patterns, then run scripts only for packages on a trust
//! allowlist (or the user's explicit opt-in).
//!
//! The scan + script gate lives entirely in Perry. The
//! fetch/resolve/lockfile/extract work is delegated to whichever installer
//! the user already has on PATH.

use anyhow::Result;
use clap::Args;
use std::path::{Path, PathBuf};

use crate::OutputFormat;

pub mod allowlist;
pub mod detect;
pub mod lifecycle;
pub mod runner;
pub mod scanner;

#[derive(Args, Debug)]
pub struct InstallArgs {
    /// Packages to install (npm-style: `pkg`, `pkg@version`, `@scope/pkg`).
    /// With no packages, installs everything declared in package.json.
    #[arg(value_name = "PKG")]
    pub packages: Vec<String>,

    /// Add to devDependencies.
    #[arg(long, short = 'D', alias = "save-dev")]
    pub save_dev: bool,

    /// Install globally.
    #[arg(long, short = 'g')]
    pub global: bool,

    /// Skip devDependencies (production install).
    #[arg(long)]
    pub production: bool,

    /// Force a specific installer backend. Auto-detect picks bun first,
    /// then npm.
    #[arg(long, value_name = "BUN|NPM")]
    pub installer: Option<String>,

    /// Skip the malware scan and only wrap the installer. Intended for
    /// debugging or when the user is intentionally vetting the project by
    /// hand. Lifecycle scripts still don't run.
    #[arg(long)]
    pub skip_scan: bool,

    /// Bypass the scan for a single package (repeat to bypass multiple).
    #[arg(long = "allow-risky", value_name = "PKG")]
    pub allow_risky: Vec<String>,

    /// Bypass the scan entirely. CI / emergency escape hatch.
    #[arg(long)]
    pub allow_risky_all: bool,

    /// Run lifecycle scripts for one extra package beyond the bundled
    /// allowlist (repeat for multiple).
    #[arg(long = "run-scripts", value_name = "PKG")]
    pub run_scripts: Vec<String>,

    /// Run all lifecycle scripts (npm-equivalent behavior). Unsafe.
    #[arg(long)]
    pub run_scripts_all: bool,

    /// Opt-in P1 freshness checks (publish age, maintainer drift). Adds
    /// one network round-trip per direct dependency.
    #[arg(long)]
    pub check_freshness: bool,

    /// Machine-readable JSON output.
    #[arg(long)]
    pub json: bool,
}

pub fn run(args: InstallArgs, _format: OutputFormat, use_color: bool) -> Result<()> {
    let installer = detect::pick(args.installer.as_deref())?;

    if !args.json {
        installer.print_banner(use_color);
    }

    runner::install(&installer, &args)?;

    if args.skip_scan {
        if !args.json {
            eprintln!("perry install: --skip-scan in effect, no scan performed");
        }
        return Ok(());
    }

    // Locate node_modules from cwd (or any parent). bun/npm always create
    // it at the project root, which is where the user invoked perry from
    // unless they explicitly cd'd into a subdir.
    let cwd = std::env::current_dir()?;
    let node_modules = match find_node_modules(&cwd) {
        Some(p) => p,
        None => {
            if !args.json {
                eprintln!(
                    "perry install: warning — node_modules/ not found after install \
                     (was --global used?); skipping scan"
                );
            }
            return Ok(());
        }
    };
    let project_root = node_modules
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| cwd.clone());

    let packages = scanner::discover_packages(&node_modules);
    let report = scanner::scan_packages(&packages, &args.allow_risky, args.allow_risky_all);
    report.write_to(&project_root)?;

    if args.json {
        // Machine-readable mode: dump the report and let exit code carry
        // the verdict.
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_summary(&report, use_color);
        if !report.findings.is_empty() {
            print_findings(&report, use_color);
        }
        print_outcome(&report, use_color);
    }

    if matches!(report.verdict, scanner::report::Verdict::Blocked) {
        // node_modules is on disk but no lifecycle scripts have run —
        // it's safe to exit non-zero here. The user can inspect
        // node_modules, then re-run with --allow-risky as appropriate.
        std::process::exit(1);
    }

    // Scan passed (Clean or Overridden) — now run lifecycle scripts
    // for packages on the trust gate.
    let summary = lifecycle::run_all(&packages, &project_root, &args)?;
    if !args.json {
        print_lifecycle_summary(&summary, use_color);
    }

    Ok(())
}

fn print_lifecycle_summary(summary: &lifecycle::RunSummary, use_color: bool) {
    if !summary.ran.is_empty() {
        if use_color {
            eprintln!(
                "\x1b[2mperry install: ran lifecycle scripts for {} allowlisted package(s): {}\x1b[0m",
                summary.ran.len(),
                summary.ran.join(", ")
            );
        } else {
            eprintln!(
                "perry install: ran lifecycle scripts for {} allowlisted package(s): {}",
                summary.ran.len(),
                summary.ran.join(", ")
            );
        }
    }
    if !summary.skipped.is_empty() {
        if use_color {
            eprintln!(
                "\x1b[33m▲\x1b[0m perry install: skipped lifecycle scripts for {} non-allowlisted \
                 package(s): {}",
                summary.skipped.len(),
                summary.skipped.join(", ")
            );
            eprintln!(
                "  add to `perry.allowScripts` in package.json, or use `--run-scripts <pkg>` to run."
            );
        } else {
            eprintln!(
                "perry install: skipped lifecycle scripts for {} non-allowlisted package(s): {}",
                summary.skipped.len(),
                summary.skipped.join(", ")
            );
            eprintln!(
                "  add to `perry.allowScripts` in package.json, or use `--run-scripts <pkg>` to run."
            );
        }
    }
}

fn print_summary(report: &scanner::report::ScanReport, use_color: bool) {
    let pkg_word = if report.package_count == 1 {
        "package"
    } else {
        "packages"
    };
    if use_color {
        eprintln!(
            "\x1b[2mperry install: scanned {} {}\x1b[0m",
            report.package_count, pkg_word
        );
    } else {
        eprintln!(
            "perry install: scanned {} {}",
            report.package_count, pkg_word
        );
    }
}

fn print_findings(report: &scanner::report::ScanReport, use_color: bool) {
    use scanner::report::Severity;
    eprintln!();
    for f in &report.findings {
        let icon = match (&f.severity, f.overridden) {
            (Severity::P0, true) => {
                if use_color {
                    "\x1b[33m▲\x1b[0m"
                } else {
                    "[OVERRIDDEN]"
                }
            }
            (Severity::P0, false) => {
                if use_color {
                    "\x1b[31m⛔\x1b[0m"
                } else {
                    "[BLOCK]"
                }
            }
            (Severity::P1, _) => {
                if use_color {
                    "\x1b[33m!\x1b[0m"
                } else {
                    "[WARN]"
                }
            }
        };
        let pkg = if use_color {
            format!("\x1b[1m{}\x1b[0m", f.package)
        } else {
            f.package.clone()
        };
        eprintln!("  {} {} — {}", icon, pkg, f.rule);
        eprintln!("     {}", f.message);
        if let Some(loc) = &f.location {
            if use_color {
                eprintln!("     \x1b[2m({})\x1b[0m", loc);
            } else {
                eprintln!("     ({})", loc);
            }
        }
        eprintln!();
    }
}

fn print_outcome(report: &scanner::report::ScanReport, use_color: bool) {
    use scanner::report::Verdict;
    let p0 = report
        .findings
        .iter()
        .filter(|f| matches!(f.severity, scanner::report::Severity::P0))
        .count();
    let blocked = report
        .findings
        .iter()
        .filter(|f| matches!(f.severity, scanner::report::Severity::P0) && !f.overridden)
        .count();
    let overridden = p0 - blocked;
    match report.verdict {
        Verdict::Clean => {
            if use_color {
                eprintln!("\x1b[32m✓\x1b[0m perry install: scan clean");
            } else {
                eprintln!("perry install: scan clean");
            }
        }
        Verdict::Overridden => {
            if use_color {
                eprintln!(
                    "\x1b[33m▲\x1b[0m perry install: {} P0 finding(s) overridden by --allow-risky, proceeding",
                    overridden
                );
            } else {
                eprintln!(
                    "perry install: {} P0 finding(s) overridden by --allow-risky, proceeding",
                    overridden
                );
            }
        }
        Verdict::Blocked => {
            let label = if use_color {
                "\x1b[1;31mBLOCKED\x1b[0m"
            } else {
                "BLOCKED"
            };
            eprintln!(
                "{} {} P0 finding(s). node_modules/ is on disk but no lifecycle \
                 scripts have run — your environment is safe.",
                label, blocked
            );
            eprintln!(
                "  Re-run with --allow-risky <pkg> to bypass per-package, or \
                 --allow-risky-all to bypass entirely."
            );
            eprintln!("  Report: .perry/install-report.json");
        }
    }
}

/// Find `node_modules/` by walking up from `start`. Duplicated from
/// `compile/resolve.rs::find_node_modules` because that one is
/// `pub(super)`; this copy lives close to where it's used so install
/// doesn't depend on compile internals.
fn find_node_modules(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        let nm = current.join("node_modules");
        if nm.is_dir() {
            return Some(nm);
        }
        if !current.pop() {
            return None;
        }
    }
}
