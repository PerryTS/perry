//! #505 — sandbox `cargo build.rs` execution for native-archive crates.
//!
//! `perry.nativeLibrary` resolution triggers `cargo build` on developer
//! machines for any source-distributed crate. Crate `build.rs` scripts
//! run with full developer privileges — most TypeScript developers
//! don't think of `bun add` as triggering arbitrary Rust code, so the
//! implicit trust escalation is invisible.
//!
//! This module wraps the cargo invocation in an OS-level sandbox
//! (macOS `sandbox-exec` for the MVP) that denies network and
//! restricts FS writes to `target/` + the user's `~/.cargo` cache
//! + `/tmp`. The legitimate `build.rs` use cases (linking system
//! libs, running `bindgen`, generating code from local fixtures)
//! work in a sandbox.
//!
//! Build-time only — **zero runtime cost**. The compiled binary is
//! the same whether the cargo invocation was sandboxed or not.
//!
//! ## Opt-in
//!
//! Off by default for backwards compatibility. Enabled per-build via
//! `PERRY_SANDBOX_BUILDRS=1` in the environment, or per-CI via the
//! same env var. A package can be exempted via the host
//! `package.json`:
//!
//! ```json
//! { "perry": { "allowUnsandboxedBuild": ["pkg-needing-network"] } }
//! ```
//!
//! Exemption is per-host (lives in the host repo's package.json) so
//! transitive deps can't bypass on their own behalf.
//!
//! ## Profile shape
//!
//! Same starting point as #506's binary-side sandbox profile, but
//! tuned for cargo:
//!
//! - `deny default`
//! - `allow file-read*` everywhere (cargo reads system libs, source
//!   trees, dep crates, etc.)
//! - `allow file-write*` to `target/`, `~/.cargo`, `~/.rustup`,
//!   `/tmp`, `/private/tmp`, `/private/var/folders` (TempDir)
//! - `deny network*`
//! - `allow process-fork` + `process-exec` (cargo spawns rustc, cc,
//!   build scripts, ld, ...)
//! - `allow sysctl-read`, `mach-lookup`, `iokit-open` (cargo +
//!   rustc need these for basic system queries)
//!
//! ## What's deferred
//!
//! - **Linux**: `landlock` + `bubblewrap` profile, optionally
//!   `unshare -n` for network denial. Tracked as #505 follow-up.
//! - **Auto-prefetch**: run `cargo fetch` outside the sandbox before
//!   the sandboxed build, so deps are present without needing
//!   network access inside the sandbox. The MVP requires the user
//!   to pre-fetch (typical CI flow) or accept that fresh-checkout
//!   builds will fail under the sandbox.
//! - **HIR-driven profile refinement**: tighter writes when the
//!   build is known not to need certain paths.

use crate::commands::compile::CompilationContext;
use std::process::Command;

/// Returns `Command::new("cargo")` for the legacy unsandboxed path,
/// or `Command::new("sandbox-exec")` with the right preamble args
/// when the sandbox is enabled AND `package_name` is not in the
/// host's `allowUnsandboxedBuild` list AND the host is macOS.
///
/// On non-macOS hosts the sandbox is a no-op (Linux follow-up).
pub(super) fn wrap_cargo_command(ctx: &CompilationContext, package_name: &str) -> Command {
    if !sandbox_enabled() {
        return Command::new("cargo");
    }
    if ctx
        .allow_unsandboxed_build
        .iter()
        .any(|p| p == package_name)
    {
        return Command::new("cargo");
    }
    // macOS sandbox-exec is the only currently-supported backend.
    // Other hosts fall through to the legacy unsandboxed path with
    // a one-time note (issued from the driver level, not here).
    if !cfg!(target_os = "macos") {
        return Command::new("cargo");
    }
    match write_macos_buildrs_profile(ctx, package_name) {
        Ok(profile_path) => {
            let cargo_path = which_cargo();
            let mut cmd = Command::new("sandbox-exec");
            cmd.arg("-f").arg(profile_path).arg(cargo_path);
            cmd
        }
        Err(_) => Command::new("cargo"),
    }
}

/// Is the build.rs sandbox enabled for this build?
fn sandbox_enabled() -> bool {
    matches!(
        std::env::var("PERRY_SANDBOX_BUILDRS"),
        Ok(v) if v == "1" || v.eq_ignore_ascii_case("true")
    )
}

/// Locate the `cargo` binary on PATH. sandbox-exec doesn't search
/// PATH the way the shell does — it runs the literal argv[0] — so
/// we need an absolute path. Falls back to `"cargo"` if `which`
/// fails (lets the OS produce a clearer error than us swallowing it).
fn which_cargo() -> std::path::PathBuf {
    if let Ok(output) = Command::new("which").arg("cargo").output() {
        if output.status.success() {
            let trimmed = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !trimmed.is_empty() {
                return std::path::PathBuf::from(trimmed);
            }
        }
    }
    std::path::PathBuf::from("cargo")
}

/// Write a per-build sandbox-exec profile to `<project>/.perry-cache/buildrs-<pkg>.sandbox`.
/// Returns the absolute path so `sandbox-exec -f <path>` can pick it up.
fn write_macos_buildrs_profile(
    ctx: &CompilationContext,
    package_name: &str,
) -> std::io::Result<std::path::PathBuf> {
    let dir = ctx.project_root.join(".perry-cache");
    std::fs::create_dir_all(&dir)?;
    // Sanitize the package name into a single path component so a
    // hostile `name: "../etc/passwd"` can't escape the cache dir.
    let safe = package_name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>();
    let path = dir.join(format!("buildrs-{}.sandbox", safe));
    std::fs::write(&path, build_macos_buildrs_profile())?;
    Ok(path)
}

/// Static profile body. Could be parameterised per-package in the
/// future (HIR-driven refinement is a #505 follow-up); for now the
/// same profile is reused across packages.
pub(super) fn build_macos_buildrs_profile() -> String {
    let mut s = String::new();
    s.push_str(";; perry build.rs sandbox profile (auto-generated, #505 MVP)\n");
    s.push_str(";;\n");
    s.push_str(";; Applied by Perry's compile driver when\n");
    s.push_str(";; PERRY_SANDBOX_BUILDRS=1 is set and the package isn't in\n");
    s.push_str(";; perry.allowUnsandboxedBuild. Tighten/relax this if your\n");
    s.push_str(";; specific build.rs needs broader access — but consider\n");
    s.push_str(";; whether the package should be in `allowUnsandboxedBuild`\n");
    s.push_str(";; instead so the exemption is recorded in the host's\n");
    s.push_str(";; package.json (auditable in code review).\n\n");

    s.push_str("(version 1)\n");
    s.push_str("(deny default)\n\n");

    // Cargo + rustc need basic process info / signal / sysctl access.
    s.push_str(";; --- process basics ---\n");
    s.push_str("(allow process-info-pidinfo)\n");
    s.push_str("(allow process-info-pidfdinfo)\n");
    s.push_str("(allow signal)\n");
    s.push_str("(allow sysctl-read)\n");
    s.push_str("(allow mach-lookup)\n");
    s.push_str("(allow iokit-open)\n\n");

    // build.rs scripts spawn rustc, cc, ld, etc. Allowed.
    s.push_str(";; --- process spawn (rustc, cc, ld, build.rs binaries) ---\n");
    s.push_str("(allow process-fork)\n");
    s.push_str("(allow process-exec)\n\n");

    // Reads: cargo source tree + system libs + crates cache.
    s.push_str(";; --- file read (system libs + source + crates cache) ---\n");
    s.push_str("(allow file-read*)\n\n");

    // Writes: tightly scoped to expected destinations. We don't try
    // to encode the project root here (sandbox-exec profile language
    // doesn't easily template, and we'd have to escape paths) — we
    // allow writes under any `target/` segment + `~/.cargo` + tmp.
    s.push_str(";; --- file write (target/, ~/.cargo, ~/.rustup, /tmp) ---\n");
    s.push_str("(allow file-write*\n");
    s.push_str("    (regex \"/target/\")\n");
    s.push_str("    (regex \"/\\\\.cargo/\")\n");
    s.push_str("    (regex \"/\\\\.rustup/\")\n");
    s.push_str("    (subpath \"/tmp\")\n");
    s.push_str("    (subpath \"/private/tmp\")\n");
    s.push_str("    (subpath \"/private/var/folders\"))\n\n");

    s.push_str(";; --- network denied (build.rs cannot phone home) ---\n");
    s.push_str(";; --- run `cargo fetch` outside the sandbox first if your ---\n");
    s.push_str(";; --- build needs to download fresh deps. ---\n");
    s.push_str("(deny network*)\n");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_denies_network_allows_build_dirs() {
        let p = build_macos_buildrs_profile();
        assert!(p.contains("(deny default)"), "default-deny missing:\n{p}");
        assert!(
            p.contains("(deny network*)"),
            "network must be denied:\n{p}"
        );
        assert!(
            p.contains("(allow process-fork)") && p.contains("(allow process-exec)"),
            "must allow rustc/cc spawn:\n{p}"
        );
        assert!(
            p.contains("(allow file-read*)"),
            "must allow file-read (system libs, source, crates):\n{p}"
        );
        assert!(
            p.contains("(regex \"/target/\")"),
            "must allow writes to target/:\n{p}"
        );
        assert!(
            p.contains("(regex \"/\\\\.cargo/\")"),
            "must allow writes to ~/.cargo:\n{p}"
        );
        assert!(
            p.contains("(regex \"/\\\\.rustup/\")"),
            "must allow writes to ~/.rustup:\n{p}"
        );
    }

    #[test]
    fn profile_header_documents_usage_and_escape_hatch() {
        let p = build_macos_buildrs_profile();
        assert!(
            p.contains("PERRY_SANDBOX_BUILDRS=1"),
            "must mention env opt-in:\n{p}"
        );
        assert!(
            p.contains("allowUnsandboxedBuild"),
            "must mention escape hatch:\n{p}"
        );
        assert!(p.contains("#505"), "must cite tracker:\n{p}");
    }

    #[test]
    fn wrap_cargo_falls_through_when_sandbox_disabled() {
        // Sandbox off → just `cargo`.
        let ctx = CompilationContext::new(std::path::PathBuf::from("/tmp"));
        std::env::remove_var("PERRY_SANDBOX_BUILDRS");
        let cmd = wrap_cargo_command(&ctx, "demo");
        assert_eq!(cmd.get_program(), "cargo");
    }

    #[test]
    fn wrap_cargo_falls_through_when_package_exempt() {
        let mut ctx = CompilationContext::new(std::path::PathBuf::from("/tmp"));
        ctx.allow_unsandboxed_build.push("demo".to_string());
        std::env::set_var("PERRY_SANDBOX_BUILDRS", "1");
        let cmd = wrap_cargo_command(&ctx, "demo");
        std::env::remove_var("PERRY_SANDBOX_BUILDRS");
        assert_eq!(cmd.get_program(), "cargo");
    }
}
