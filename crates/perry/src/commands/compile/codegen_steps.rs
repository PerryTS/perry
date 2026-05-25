//! #1680 (Phase 2 of #1677) — run host-declared build-time codegen steps.
//!
//! Several codegen libraries already ship an eval-free path built for CSP
//! environments that emits plain source at build time (`ajv/standalone`,
//! `prisma generate`, `drizzle-kit introspect`, `kysely-codegen`, the Vue
//! SFC compiler, …). Where that exists, Perry consumes the standalone
//! output instead of running a JIT at runtime — zero new evaluation infra,
//! highest ROI.
//!
//! The convention: the host `package.json` declares the build commands
//! under `perry.codegen`; Perry runs them (in the package.json's directory)
//! *before* module collection, so the generated, eval-free source is on
//! disk for the normal native compile path to pick up. Steps are read only
//! from the host package.json — never a dependency's — the same trust
//! boundary as `perry.compilePackages` (a transitive dep can't smuggle in a
//! build command). `--no-codegen` / `PERRY_SKIP_CODEGEN=1` skips the steps
//! for reproducible / sandboxed builds whose generated output is committed.

use std::process::Command;

use anyhow::{anyhow, bail, Result};

use super::CompilationContext;
use crate::OutputFormat;

/// Run every `perry.codegen` step declared in the host package.json, in
/// declaration order, in `ctx.codegen_dir` (falling back to the project
/// root). `skip` short-circuits the whole pass (driven by `--no-codegen` /
/// `PERRY_SKIP_CODEGEN`). Bails on the first command that fails to spawn or
/// exits non-zero, surfacing its captured stdout/stderr so the failure is
/// actionable.
pub(super) fn run_codegen_steps(
    ctx: &CompilationContext,
    skip: bool,
    format: OutputFormat,
) -> Result<()> {
    if ctx.codegen_steps.is_empty() {
        return Ok(());
    }
    if skip {
        if matches!(format, OutputFormat::Text) {
            println!(
                "  Skipping {} perry.codegen step(s) (--no-codegen / PERRY_SKIP_CODEGEN)",
                ctx.codegen_steps.len()
            );
        }
        return Ok(());
    }

    let cwd = ctx
        .codegen_dir
        .clone()
        .unwrap_or_else(|| ctx.project_root.clone());

    for step in &ctx.codegen_steps {
        let label = step.label.as_deref().unwrap_or(step.command.as_str());
        if matches!(format, OutputFormat::Text) {
            println!("  Codegen: {label}");
        }
        let output = shell_command(&step.command)
            .current_dir(&cwd)
            .output()
            .map_err(|e| {
                anyhow!(
                    "failed to spawn perry.codegen step `{}`: {}",
                    step.command,
                    e
                )
            })?;
        if !output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "perry.codegen step failed: `{cmd}`\n  cwd: {cwd}\n  exit: {status}\n\
                 --- stdout ---\n{stdout}\n--- stderr ---\n{stderr}",
                cmd = step.command,
                cwd = cwd.display(),
                status = output.status,
                stdout = stdout.trim_end(),
                stderr = stderr.trim_end(),
            );
        }
    }
    Ok(())
}

/// Whether `PERRY_SKIP_CODEGEN` is set to a truthy value.
pub(super) fn skip_from_env() -> bool {
    match std::env::var("PERRY_SKIP_CODEGEN") {
        Ok(v) => {
            let v = v.trim().to_ascii_lowercase();
            !matches!(v.as_str(), "" | "0" | "off" | "false" | "no")
        }
        Err(_) => false,
    }
}

/// Build a shell command so full command strings (`node gen.mjs && …`)
/// work as written. Perry's supported compile hosts are unix; Windows uses
/// `cmd /C` for parity but is untested here.
#[cfg(not(windows))]
fn shell_command(cmd: &str) -> Command {
    let mut c = Command::new("sh");
    c.arg("-c").arg(cmd);
    c
}

#[cfg(windows)]
fn shell_command(cmd: &str) -> Command {
    let mut c = Command::new("cmd");
    c.arg("/C").arg(cmd);
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::compile::CodegenStep;

    fn ctx_with_steps(dir: &std::path::Path, steps: Vec<CodegenStep>) -> CompilationContext {
        let mut ctx = CompilationContext::new(dir.to_path_buf());
        ctx.codegen_dir = Some(dir.to_path_buf());
        ctx.codegen_steps = steps;
        ctx
    }

    #[test]
    fn runs_step_in_codegen_dir_and_produces_output() {
        let dir = std::env::temp_dir().join(format!("perry_codegen_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ctx = ctx_with_steps(
            &dir,
            vec![CodegenStep {
                label: Some("write sentinel".to_string()),
                // Relative path → resolves against codegen_dir.
                command: "printf done > generated.txt".to_string(),
            }],
        );
        run_codegen_steps(&ctx, false, OutputFormat::Json).unwrap();
        let produced = std::fs::read_to_string(dir.join("generated.txt")).unwrap();
        assert_eq!(produced, "done");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn skip_does_not_run_steps() {
        let dir = std::env::temp_dir().join(format!("perry_codegen_skip_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ctx = ctx_with_steps(
            &dir,
            vec![CodegenStep {
                label: None,
                command: "printf done > should_not_exist.txt".to_string(),
            }],
        );
        run_codegen_steps(&ctx, true, OutputFormat::Json).unwrap();
        assert!(!dir.join("should_not_exist.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn failing_step_bails_with_diagnostics() {
        let dir = std::env::temp_dir().join(format!("perry_codegen_fail_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ctx = ctx_with_steps(
            &dir,
            vec![CodegenStep {
                label: None,
                command: "echo boom 1>&2; exit 3".to_string(),
            }],
        );
        let err = run_codegen_steps(&ctx, false, OutputFormat::Json).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("perry.codegen step failed"));
        assert!(msg.contains("boom"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_steps_is_noop() {
        let dir = std::env::temp_dir();
        let ctx = CompilationContext::new(dir);
        run_codegen_steps(&ctx, false, OutputFormat::Json).unwrap();
    }
}
