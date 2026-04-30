//! Doctor command - check environment and dependencies

use anyhow::Result;
use clap::Args;
use console::{style, Emoji};
use std::path::PathBuf;
use std::process::Command;

use crate::update_checker;
use crate::OutputFormat;

#[derive(Args, Debug)]
pub struct DoctorArgs {
    /// Run checks silently and only report failures
    #[arg(long)]
    pub quiet: bool,
}

static CHECK: Emoji<'_, '_> = Emoji("✓ ", "[OK] ");
static CROSS: Emoji<'_, '_> = Emoji("✗ ", "[FAIL] ");
static WARN: Emoji<'_, '_> = Emoji("⚠ ", "[WARN] ");

struct CheckResult {
    name: String,
    status: CheckStatus,
    details: Option<String>,
}

enum CheckStatus {
    Ok,
    Warning,
    Error,
}

#[cfg(target_os = "windows")]
fn msvc_vswhere_installation_path_args() -> [&'static str; 8] {
    [
        "-products",
        "*",
        // Without the VC tools filter, `-latest` can select Management Studio.
        "-requires",
        "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
        "-latest",
        "-property",
        "installationPath",
        "-nologo",
    ]
}

fn check_perry_version() -> CheckResult {
    CheckResult {
        name: "perry version".to_string(),
        status: CheckStatus::Ok,
        details: Some(env!("CARGO_PKG_VERSION").to_string()),
    }
}

fn check_clang() -> CheckResult {
    match perry_codegen::linker::find_clang() {
        Some(path) => {
            let version = Command::new(&path)
                .arg("--version")
                .output()
                .ok()
                .and_then(|out| {
                    let s = String::from_utf8_lossy(&out.stdout).to_string();
                    s.lines().next().map(|l| l.to_string())
                })
                .unwrap_or_else(|| path.display().to_string());
            CheckResult {
                name: "clang (LLVM codegen)".to_string(),
                status: CheckStatus::Ok,
                details: Some(version),
            }
        }
        None => {
            let hint = if cfg!(windows) {
                "not found - install with `winget install LLVM.LLVM` (or choco/scoop install llvm) or set PERRY_LLVM_CLANG"
            } else if cfg!(target_os = "macos") {
                "not found - install with `brew install llvm` or `xcode-select --install`, or set PERRY_LLVM_CLANG"
            } else {
                "not found - install via your package manager (apt/dnf/pacman install clang) or set PERRY_LLVM_CLANG"
            };
            CheckResult {
                name: "clang (LLVM codegen)".to_string(),
                status: CheckStatus::Error,
                details: Some(hint.to_string()),
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn find_xwin_sysroot() -> Option<PathBuf> {
    let explicit = std::env::var("PERRY_WINDOWS_SYSROOT")
        .ok()
        .map(PathBuf::from);
    let default = dirs::data_local_dir().map(|p| p.join("perry").join("windows-sdk"));
    for candidate in [explicit, default].into_iter().flatten() {
        if candidate.join("crt").join("lib").join("x86_64").exists()
            || candidate.join("crt").join("lib").join("x64").exists()
        {
            return Some(candidate);
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn find_lld_link() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("PERRY_LLD_LINK") {
        let candidate = PathBuf::from(p);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    let standalone = PathBuf::from(r"C:\Program Files\LLVM\bin\lld-link.exe");
    if standalone.exists() {
        return Some(standalone);
    }
    None
}

fn check_system_linker() -> CheckResult {
    #[cfg(target_os = "windows")]
    {
        // Two valid toolchains — either suffices. Prefer xwin when present
        // (matches compile.rs precedence: user ran `perry setup windows` ⇒ opted in).
        let xwin = find_xwin_sysroot();
        let lld = find_lld_link();
        if let (Some(sysroot), Some(lld_path)) = (&xwin, &lld) {
            return CheckResult {
                name: "system linker (lld-link + xwin sysroot)".to_string(),
                status: CheckStatus::Ok,
                details: Some(format!("{} + {}", lld_path.display(), sysroot.display())),
            };
        }

        // Fall back to MSVC detection
        let mut linker = PathBuf::from("link.exe");
        let vswhere =
            PathBuf::from(r"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe");
        if vswhere.exists() {
            if let Ok(output) = Command::new(&vswhere)
                .args(msvc_vswhere_installation_path_args())
                .output()
            {
                let install_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !install_path.is_empty() {
                    let msvc_dir = PathBuf::from(&install_path).join(r"VC\Tools\MSVC");
                    if let Ok(entries) = std::fs::read_dir(&msvc_dir) {
                        let mut versions: Vec<_> = entries.filter_map(|e| e.ok()).collect();
                        versions.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
                        for entry in versions {
                            let link = entry.path().join(r"bin\Hostx64\x64\link.exe");
                            if link.exists() {
                                linker = link;
                                break;
                            }
                        }
                    }
                }
            }
        }
        let output = Command::new(&linker).arg("/NOLOGO").output();
        match output {
            Ok(_) => CheckResult {
                name: "system linker (MSVC link.exe)".to_string(),
                status: CheckStatus::Ok,
                details: Some(linker.display().to_string()),
            },
            Err(_) => {
                // Neither path is complete. Report partial state when possible.
                let hint = match (xwin, lld) {
                    (Some(sysroot), None) => format!(
                        "xwin sysroot at {} but lld-link.exe missing — run `winget install LLVM.LLVM`",
                        sysroot.display()
                    ),
                    (None, Some(lld_path)) => format!(
                        "lld-link at {} but no Windows SDK libs — run `perry setup windows`",
                        lld_path.display()
                    ),
                    _ => String::from(
                        "no Windows linker. Install EITHER (lightweight ~1.5 GB):\n      \
                         winget install LLVM.LLVM && perry setup windows\n      \
                         OR (MSVC ~8 GB): Visual Studio Installer → Modify → \"Desktop development with C++\""
                    ),
                };
                CheckResult {
                    name: "system linker".to_string(),
                    status: CheckStatus::Error,
                    details: Some(hint),
                }
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let output = Command::new("cc").arg("--version").output();
        match output {
            Ok(output) if output.status.success() => {
                let version = String::from_utf8_lossy(&output.stdout);
                let first_line = version.lines().next().unwrap_or("unknown");
                CheckResult {
                    name: "system linker (cc)".to_string(),
                    status: CheckStatus::Ok,
                    details: Some(first_line.to_string()),
                }
            }
            Ok(_) => CheckResult {
                name: "system linker (cc)".to_string(),
                status: CheckStatus::Error,
                details: Some("cc command failed".to_string()),
            },
            Err(e) => CheckResult {
                name: "system linker (cc)".to_string(),
                status: CheckStatus::Error,
                details: Some(format!("cc not found: {}", e)),
            },
        }
    }
}

fn check_runtime_library() -> CheckResult {
    // Delegate to the same search machinery `perry compile` uses, so the
    // doctor view stays in sync with the linker's actual lookup paths
    // (env-var overrides, WinGet Packages dir, brew/usr-local, etc).
    let lib_name = if cfg!(target_os = "windows") {
        "perry_runtime.lib"
    } else {
        "libperry_runtime.a"
    };
    if let Some(path) = crate::commands::compile::find_library(lib_name, None) {
        return CheckResult {
            name: "runtime library".to_string(),
            status: CheckStatus::Ok,
            details: Some(path.display().to_string()),
        };
    }
    CheckResult {
        name: "runtime library".to_string(),
        status: CheckStatus::Warning,
        details: Some("not found - run: cargo build --release -p perry-runtime".to_string()),
    }
}

fn check_update_available() -> CheckResult {
    match update_checker::check_cached_status() {
        update_checker::UpdateStatus::UpdateAvailable { latest, .. } => CheckResult {
            name: "update status".to_string(),
            status: CheckStatus::Warning,
            details: Some(format!("v{} available — run `perry update`", latest)),
        },
        update_checker::UpdateStatus::UpToDate => CheckResult {
            name: "update status".to_string(),
            status: CheckStatus::Ok,
            details: Some("up to date".to_string()),
        },
        update_checker::UpdateStatus::CheckFailed => CheckResult {
            name: "update status".to_string(),
            status: CheckStatus::Ok,
            details: Some("no cached info (run `perry update --check-only`)".to_string()),
        },
    }
}

fn check_project_config() -> CheckResult {
    let config_path = PathBuf::from("perry.toml");
    if config_path.exists() {
        CheckResult {
            name: "project config (perry.toml)".to_string(),
            status: CheckStatus::Ok,
            details: Some("found".to_string()),
        }
    } else {
        CheckResult {
            name: "project config (perry.toml)".to_string(),
            status: CheckStatus::Warning,
            details: Some("not found - run: perry init".to_string()),
        }
    }
}

pub fn run(args: DoctorArgs, format: OutputFormat, use_color: bool) -> Result<()> {
    let checks = vec![
        check_perry_version(),
        check_update_available(),
        check_clang(),
        check_system_linker(),
        check_runtime_library(),
        check_project_config(),
    ];

    let mut has_errors = false;
    let mut has_warnings = false;

    match format {
        OutputFormat::Text => {
            if !args.quiet {
                println!("Perry Doctor\n");
                println!("Environment Checks");
                println!("──────────────────");
            }

            for check in &checks {
                let (emoji, color_fn): (_, fn(&str) -> console::StyledObject<&str>) =
                    match check.status {
                        CheckStatus::Ok => (CHECK, |s| style(s).green()),
                        CheckStatus::Warning => {
                            has_warnings = true;
                            (WARN, |s| style(s).yellow())
                        }
                        CheckStatus::Error => {
                            has_errors = true;
                            (CROSS, |s| style(s).red())
                        }
                    };

                let status_str = match check.status {
                    CheckStatus::Ok => "OK",
                    CheckStatus::Warning => "WARN",
                    CheckStatus::Error => "FAIL",
                };

                if args.quiet && matches!(check.status, CheckStatus::Ok) {
                    continue;
                }

                if use_color {
                    print!("  {}{}: ", emoji, check.name);
                    if let Some(ref details) = check.details {
                        println!("{}", color_fn(details));
                    } else {
                        println!("{}", color_fn(status_str));
                    }
                } else {
                    print!("  [{}] {}: ", status_str, check.name);
                    if let Some(ref details) = check.details {
                        println!("{}", details);
                    } else {
                        println!();
                    }
                }
            }

            if !args.quiet {
                println!();
                if has_errors {
                    println!("Some checks failed. Please fix the issues above.");
                } else if has_warnings {
                    println!("All critical checks passed with some warnings.");
                } else {
                    println!("All checks passed!");
                }
            }
        }
        OutputFormat::Json => {
            let results: Vec<_> = checks
                .iter()
                .map(|c| {
                    serde_json::json!({
                        "name": c.name,
                        "status": match c.status {
                            CheckStatus::Ok => "ok",
                            CheckStatus::Warning => "warning",
                            CheckStatus::Error => "error",
                        },
                        "details": c.details,
                    })
                })
                .collect();

            let output = serde_json::json!({
                "success": !has_errors,
                "checks": results,
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
    }

    if has_errors {
        std::process::exit(1);
    }

    Ok(())
}
