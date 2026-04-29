//! Driver: write `.ll` text to disk, shell out to `clang -c` to produce an
//! object file, and return its bytes.
//!
//! This is the seam that lets Perry's existing linking pipeline (nm scan +
//! `cc` invocation in `crates/perry/src/commands/compile.rs`) stay unchanged.
//! Both backends produce the same artifact — an object file as `Vec<u8>` —
//! so the rest of the compile pipeline doesn't care which one ran.

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use anyhow::{anyhow, Context, Result};

/// Cached result of the pre-flight clang probe — evaluated once per process.
/// `Some(default_triple)` if the probe succeeded, `None` if it failed.
static CLANG_PROBE: OnceLock<Option<String>> = OnceLock::new();

/// Compile LLVM IR text to an object file using the system `clang`, returning
/// the object file bytes.
///
/// We write the `.ll` to a temp file (LLVM text is big and clang reads it
/// more reliably from disk than from stdin), invoke `clang -c`, read the
/// resulting `.o`, and clean up both on success. On failure the temp files
/// are left behind for debugging — the caller can `grep /tmp/perry_llvm_*`.
pub fn compile_ll_to_object(ll_text: &str, target_triple: Option<&str>) -> Result<Vec<u8>> {
    let tmp_dir = env::temp_dir();
    let pid = std::process::id();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let ll_path = tmp_dir.join(format!("perry_llvm_{}_{}.ll", pid, nonce));
    let obj_path = tmp_dir.join(format!("perry_llvm_{}_{}.o", pid, nonce));

    {
        let mut f = fs::File::create(&ll_path)
            .with_context(|| format!("Failed to create temp .ll file at {}", ll_path.display()))?;
        f.write_all(ll_text.as_bytes())?;
    }

    let clang = find_clang().context(if cfg!(windows) {
        "clang not found. Install LLVM with one of:\n\
         \n\
         \x20   winget install LLVM.LLVM       (Windows Package Manager)\n\
         \x20   choco install llvm             (Chocolatey)\n\
         \x20   scoop install llvm             (Scoop)\n\
         \n\
         or download the installer from https://github.com/llvm/llvm-project/releases\n\
         (look for LLVM-<version>-win64.exe). After installation, open a new terminal\n\
         so the updated PATH takes effect, or set PERRY_LLVM_CLANG to the full path of\n\
         clang.exe. Run `perry doctor` to verify the install."
    } else if cfg!(target_os = "macos") {
        "clang not found. Install LLVM with `brew install llvm` or install Xcode \
         command-line tools with `xcode-select --install`. Or set PERRY_LLVM_CLANG \
         to the path of clang. Run `perry doctor` to verify the install."
    } else {
        "clang not found in PATH. Install LLVM/clang via your package manager \
         (e.g. `apt install clang`, `dnf install clang`, `pacman -S clang`) or set \
         PERRY_LLVM_CLANG to the path of clang. Run `perry doctor` to verify the install."
    })?;

    // Pre-flight probe: capture clang's default Target: line once per process,
    // so we can warn early if it disagrees with the IR's triple in a way that
    // historically broke Windows builds. The actual build still succeeds via
    // the explicit -target pin below — the probe is purely informational.
    let effective_triple_for_probe = target_triple
        .map(|s| s.to_string())
        .unwrap_or_else(crate::codegen::default_target_triple);
    probe_clang_default_triple(&clang, &effective_triple_for_probe);

    let mut cmd = Command::new(&clang);
    cmd.arg("-c")
        // -O3 unlocks LLVM's auto-vectorizer, aggressive inlining, and
        // better SLP / loop unrolling. The compile-time cost vs -O2 is
        // small for typical user programs (<1s of overhead) compared
        // to the runtime perf wins on tight loops.
        .arg("-O3")
        // Include DWARF debug info so crash symbolicators can map
        // addresses back to function names. Only enabled when
        // PERRY_DEBUG_SYMBOLS=1 is set — otherwise omitted to keep
        // binaries small.
        .args(if std::env::var("PERRY_DEBUG_SYMBOLS").is_ok() {
            vec!["-g"]
        } else {
            vec![]
        })
        // We want LLVM to reassociate f64 ops (for loop unrolling)
        // but NOT to assume NaN never occurs — Perry's NaN-boxing uses
        // NaN bit patterns for ALL non-number values (strings, objects,
        // null, undefined, booleans). -ffast-math includes
        // -ffinite-math-only which tells LLVM NaN never happens,
        // causing it to replace NaN-boxed constants (TAG_NULL, etc.)
        // with 0.0. Use individual flags instead:
        // -funsafe-math-optimizations: allows reassociation + reciprocal
        // -fno-math-errno: skip errno checks on math functions
        // (Do NOT use -ffinite-math-only or -ffast-math)
        .arg("-fno-math-errno");
    // Native CPU tuning: only when building for the host. The flag name
    // (`-mcpu` vs `-march`) is also arch-specific, and clang rejects
    // `-mcpu=` for x86 targets and `-march=` for arm targets — so when
    // cross-compiling we skip it entirely and let clang's `-target`
    // default suffice. (Without this guard, an aarch64 macOS host
    // cross-building for `x86_64-unknown-linux-gnu` would pass
    // `-mcpu=native` to a clang invocation aimed at x86, which fails
    // with `unsupported option '-mcpu='`.)
    if target_triple.is_none() {
        cmd.arg(if cfg!(target_arch = "aarch64") {
            "-mcpu=native"
        } else {
            "-march=native"
        });
    }
    cmd.arg(&ll_path).arg("-o").arg(&obj_path);
    // Always pass -target. Clang's behavior on a `.ll` file is "use my own
    // default target, override the module's stated triple if it differs"
    // (you can see the `warning: overriding the module target triple` log
    // when this happens). On a host where the discovered clang's default
    // is non-msvc — typically MinGW-flavored clang from MSYS2, Strawberry
    // Perl, an Anaconda env, or a Rust GNU toolchain LLVM bundle — that
    // override silently turns Perry's stated `x86_64-pc-windows-msvc`
    // module into a windows-gnu/mingw32 object. LLVM's mingw32 COFF
    // emitter then injects a `__main` reference (a libgcc/MinGW C++
    // static-init stub) into our generated `main()`. lld-link / link.exe
    // are MSVC-flavored — they don't have `__main`, so the link bombs
    // with `LNK2019: unresolved external symbol __main referenced in
    // function main`. Pinning -target to the IR's actual triple (or the
    // host default when target is None) makes clang trust the IR and
    // skips the override path.
    let effective_triple = target_triple
        .map(|s| s.to_string())
        .unwrap_or_else(crate::codegen::default_target_triple);
    cmd.arg("-target").arg(&effective_triple);

    log::debug!("perry-codegen: {:?}", cmd);
    let output = cmd
        .output()
        .with_context(|| format!("Failed to invoke {}", clang.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Surface the clang environment alongside the failure so the user
        // doesn't have to chase a cryptic LNK2019 / "unresolved external
        // symbol" up the toolchain. We probe `clang --version` once on
        // failure so the working path stays single-shellout.
        let clang_version = Command::new(&clang)
            .arg("--version")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "(unable to query --version)".to_string());
        let hint = build_clang_failure_hint(&stderr, &clang_version, &effective_triple);
        return Err(anyhow!(
            "clang -c failed (status={}).\n\
             clang:           {}\n\
             clang --version: {}\n\
             requested -target: {}\n\
             LLVM IR left at: {}\n\
             \n\
             stderr:\n{}\n\
             {}",
            output.status,
            clang.display(),
            clang_version.lines().next().unwrap_or("?"),
            effective_triple,
            ll_path.display(),
            stderr,
            hint
        ));
    }

    let bytes = fs::read(&obj_path)
        .with_context(|| format!("Failed to read clang output at {}", obj_path.display()))?;

    // Clean up temp files on success — unless PERRY_LLVM_KEEP_IR is set, in
    // which case we leave the .ll around for debugging and print the path.
    let keep = env::var_os("PERRY_LLVM_KEEP_IR").is_some();
    if keep {
        eprintln!("[perry-codegen] kept LLVM IR: {}", ll_path.display());
        eprintln!("[perry-codegen] kept object:  {}", obj_path.display());
    } else {
        let _ = fs::remove_file(&ll_path);
        let _ = fs::remove_file(&obj_path);
    }

    Ok(bytes)
}

/// Once-per-process probe of clang's default `Target:` line. When the
/// default disagrees with the triple Perry is about to pass via `-target`
/// in a way that historically broke builds (specifically: a non-msvc
/// clang default on a Windows host targeting msvc), print a one-line
/// informational note pointing the user at `PERRY_LLVM_CLANG` /
/// `LLVM.LLVM`. The build itself proceeds normally — this is just a
/// heads-up so a "tricky" failure surfaces as a clear note up front
/// instead of a downstream link error.
///
/// Suppress with `PERRY_NO_CLANG_PROBE=1` (CI / scripted builds).
fn probe_clang_default_triple(clang: &Path, requested_triple: &str) {
    if env::var_os("PERRY_NO_CLANG_PROBE").is_some() {
        return;
    }
    let default_triple = CLANG_PROBE
        .get_or_init(|| {
            let out = Command::new(clang).arg("--version").output().ok()?;
            let text = String::from_utf8(out.stdout).ok()?;
            text.lines()
                .find(|l| l.trim_start().starts_with("Target:"))
                .map(|l| {
                    l.trim_start()
                        .trim_start_matches("Target:")
                        .trim()
                        .to_string()
                })
        })
        .as_deref();

    let Some(default) = default_triple else {
        return;
    };

    // Only warn when the host is Windows and clang's default is GNU/MinGW
    // but we're targeting msvc. Any other mismatch (e.g. cross-compile)
    // is intentional and not a sign of a broken install.
    let host_is_windows = cfg!(target_os = "windows");
    let want_msvc = requested_triple.contains("windows-msvc");
    let have_gnu = default.contains("windows-gnu")
        || default.contains("mingw")
        || default.contains("w64-mingw");
    if host_is_windows && want_msvc && have_gnu {
        eprintln!(
            "  note: clang default is `{}` (MinGW/GNU); Perry is forcing -target {} \
             so the link stays MSVC-flavored.\n        \
             If anything below fails, install msvc-default LLVM (winget install LLVM.LLVM) \
             or set PERRY_LLVM_CLANG.",
            default, requested_triple
        );
    }
}

/// Build a human-readable hint paragraph appended to a `clang -c` failure.
/// Pattern-matches the stderr against the failure shapes we know about and
/// produces an actionable next step, so a user reading the error doesn't
/// have to interpret raw lld-link / clang messages.
fn build_clang_failure_hint(stderr: &str, clang_version: &str, requested_triple: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    let lower = stderr.to_lowercase();
    let version_line = clang_version.lines().next().unwrap_or("");
    let clang_default_triple = clang_version
        .lines()
        .find(|l| l.trim_start().starts_with("Target:"))
        .map(|l| {
            l.trim_start()
                .trim_start_matches("Target:")
                .trim()
                .to_string()
        });

    let mingw_clang = clang_default_triple
        .as_deref()
        .map(|t| t.contains("windows-gnu") || t.contains("mingw") || t.contains("w64-mingw"))
        .unwrap_or(false);

    if cfg!(target_os = "windows") && mingw_clang {
        lines.push(format!(
            "Hint: the clang on PATH defaults to {} (a MinGW/GNU toolchain). \
             Perry now pins -target to {} so the .o is msvc-flavored, but if your \
             clang install lacks the msvc backend support, pick a clang built for msvc:",
            clang_default_triple
                .as_deref()
                .unwrap_or("a non-msvc target"),
            requested_triple
        ));
        lines.push("  - winget install LLVM.LLVM        (Windows Package Manager)".to_string());
        lines.push("  - choco install llvm              (Chocolatey)".to_string());
        lines.push(
            "  - https://github.com/llvm/llvm-project/releases (LLVM-<ver>-win64.exe)".to_string(),
        );
        lines.push(
            "Then either put it first on PATH, or set PERRY_LLVM_CLANG to its full path."
                .to_string(),
        );
    } else if lower.contains("overriding the module target triple") {
        lines.push(format!(
            "Hint: clang ({}) is overriding the module target triple. \
             Perry passes -target {} explicitly; if you see this message after the fix, \
             your clang may not support that target — install LLVM.LLVM or set PERRY_LLVM_CLANG.",
            version_line, requested_triple
        ));
    } else if lower.contains("unable to find library") || lower.contains("library not found") {
        lines.push(format!(
            "Hint: clang couldn't find a system library. Check that the platform SDK is installed \
             (Visual Studio Build Tools on Windows, Xcode CLT on macOS, libc6-dev/build-essential \
             on Linux). Requested target: {}.",
            requested_triple
        ));
    } else {
        lines.push(format!(
            "If the failure is a triple/ABI mismatch, set PERRY_LLVM_CLANG to a clang whose \
             default Target: matches {} (run `perry doctor` to verify).",
            requested_triple
        ));
    }
    lines.join("\n")
}

pub fn find_clang() -> Option<PathBuf> {
    // Honor explicit override first — useful on systems with multiple clang
    // installs (e.g. Homebrew LLVM vs Xcode).
    if let Ok(p) = env::var("PERRY_LLVM_CLANG") {
        let candidate = PathBuf::from(p);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    // Check PATH (with .exe extension handling on Windows).
    if which("clang") {
        return Some(PathBuf::from("clang"));
    }
    // Check well-known install locations.
    #[cfg(windows)]
    {
        // Standalone LLVM installer (llvm.org)
        let standalone = PathBuf::from(r"C:\Program Files\LLVM\bin\clang.exe");
        if standalone.exists() {
            return Some(standalone);
        }
        // MSVC Build Tools bundled clang (via "C++ Clang Compiler" component)
        if let Some(path) = find_msvc_bundled_clang() {
            return Some(path);
        }
    }
    #[cfg(not(windows))]
    {
        // Homebrew on macOS, ROCm / distro LLVM on Linux.
        for prefix in &[
            "/opt/homebrew/opt/llvm/bin",
            "/usr/local/opt/llvm/bin",
            "/usr/lib64/rocm/llvm/bin",
            "/usr/lib/llvm-19/bin",
            "/usr/lib/llvm-18/bin",
            "/usr/lib/llvm-17/bin",
        ] {
            let candidate = PathBuf::from(prefix).join("clang");
            if candidate.exists() && is_executable(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// Search for clang.exe bundled with Visual Studio Build Tools / Community.
/// The "C++ Clang Compiler for Windows" workload component installs it at:
///   <VS install>/VC/Tools/Llvm/x64/bin/clang.exe
#[cfg(windows)]
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

#[cfg(windows)]
fn find_msvc_bundled_clang() -> Option<PathBuf> {
    let vswhere_paths = [
        PathBuf::from(r"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe"),
        PathBuf::from(r"C:\Program Files\Microsoft Visual Studio\Installer\vswhere.exe"),
    ];
    for vswhere in &vswhere_paths {
        if !vswhere.exists() {
            continue;
        }
        let output = std::process::Command::new(vswhere)
            .args(msvc_vswhere_installation_path_args())
            .output()
            .ok()?;
        let install_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if install_path.is_empty() {
            continue;
        }
        // Check x64 first, then ARM64
        for arch in &["x64", "ARM64"] {
            let candidate = PathBuf::from(&install_path)
                .join("VC")
                .join("Tools")
                .join("Llvm")
                .join(arch)
                .join("bin")
                .join("clang.exe");
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    None
}

fn which(name: &str) -> bool {
    let path_var = match env::var_os("PATH") {
        Some(p) => p,
        None => return false,
    };
    for dir in env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.exists() && is_executable(&candidate) {
            return true;
        }
        // On Windows, executables have .exe extension
        #[cfg(windows)]
        {
            let with_exe = dir.join(format!("{}.exe", name));
            if with_exe.exists() && is_executable(&with_exe) {
                return true;
            }
        }
    }
    false
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(p)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.exists()
}

// ---------------------------------------------------------------------------
// Bitcode link pipeline (Phase J)
// ---------------------------------------------------------------------------

/// Find an LLVM tool (llvm-link, opt, llc, llvm-as) on the system.
fn find_llvm_tool(tool: &str) -> Option<PathBuf> {
    let env_key = format!("PERRY_LLVM_{}", tool.to_uppercase().replace('-', "_"));
    if let Ok(p) = env::var(&env_key) {
        let candidate = PathBuf::from(p);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    for prefix in &[
        "/opt/homebrew/opt/llvm/bin",
        "/usr/local/opt/llvm/bin",
        "/usr/lib64/rocm/llvm/bin",
        "/usr/lib/llvm-19/bin",
        "/usr/lib/llvm-18/bin",
        "/usr/lib/llvm-17/bin",
    ] {
        let candidate = PathBuf::from(prefix).join(tool);
        if candidate.exists() && is_executable(&candidate) {
            return Some(candidate);
        }
    }
    if which(tool) {
        return Some(PathBuf::from(tool));
    }
    None
}

/// Whole-program bitcode link pipeline.
///
/// Converts user `.ll` files to `.bc`, merges them with the runtime/stdlib
/// bitcode via `llvm-link`, runs `opt -O3`, then `llc -filetype=obj` to
/// produce a single object file. Returns the path to that `.o`.
pub fn bitcode_link_pipeline(
    user_ll_files: &[PathBuf],
    runtime_bc: &Path,
    stdlib_bc: Option<&Path>,
    extra_bc: &[PathBuf],
    target_triple: Option<&str>,
) -> Result<PathBuf> {
    let llvm_as = find_llvm_tool("llvm-as")
        .ok_or_else(|| anyhow!("llvm-as not found (required for bitcode link)"))?;
    let llvm_link = find_llvm_tool("llvm-link")
        .ok_or_else(|| anyhow!("llvm-link not found (required for bitcode link)"))?;
    let opt_tool = find_llvm_tool("opt")
        .ok_or_else(|| anyhow!("opt not found (required for bitcode link)"))?;
    let llc = find_llvm_tool("llc")
        .ok_or_else(|| anyhow!("llc not found (required for bitcode link)"))?;

    let tmp_dir = env::temp_dir();
    let pid = std::process::id();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let prefix = format!("perry_bc_{}_{}", pid, nonce);
    let keep = env::var_os("PERRY_LLVM_KEEP_IR").is_some();
    let mut intermediates: Vec<PathBuf> = Vec::new();

    // Step 1: llvm-as each .ll → .bc
    let mut user_bc_files: Vec<PathBuf> = Vec::new();
    for (i, ll_file) in user_ll_files.iter().enumerate() {
        let bc_path = tmp_dir.join(format!("{}_{}.bc", prefix, i));
        let output = Command::new(&llvm_as)
            .arg(ll_file)
            .arg("-o")
            .arg(&bc_path)
            .output()
            .with_context(|| format!("Failed to invoke llvm-as on {}", ll_file.display()))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!(
                "llvm-as failed on {} (status={}):\n{}",
                ll_file.display(),
                output.status,
                stderr
            ));
        }
        intermediates.push(bc_path.clone());
        user_bc_files.push(bc_path);
    }

    // Step 2: llvm-link all bitcode into one module.
    // perry-stdlib re-exports/wraps some perry-runtime symbols, so we
    // pass the stdlib as `--override` to let its definitions win.
    let linked_bc = tmp_dir.join(format!("{}_linked.bc", prefix));
    {
        let mut cmd = Command::new(&llvm_link);
        for bc in &user_bc_files {
            cmd.arg(bc);
        }
        cmd.arg(runtime_bc);
        if let Some(stdlib) = stdlib_bc {
            cmd.arg("--override").arg(stdlib);
        }
        for bc in extra_bc {
            cmd.arg(bc);
        }
        cmd.arg("-o").arg(&linked_bc);
        log::debug!("perry-codegen bitcode-link: {:?}", cmd);
        let output = cmd.output().context("Failed to invoke llvm-link")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!(
                "llvm-link failed (status={}):\n{}",
                output.status,
                stderr
            ));
        }
    }
    intermediates.push(linked_bc.clone());

    // Step 3: opt -O3
    let opt_bc = tmp_dir.join(format!("{}_opt.bc", prefix));
    {
        let mut cmd = Command::new(&opt_tool);
        cmd.arg("-O3").arg(&linked_bc).arg("-o").arg(&opt_bc);
        log::debug!("perry-codegen opt: {:?}", cmd);
        let output = cmd.output().context("Failed to invoke opt")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!(
                "opt -O3 failed (status={}):\n{}",
                output.status,
                stderr
            ));
        }
    }
    intermediates.push(opt_bc.clone());

    // Step 4: llc -filetype=obj → .o
    let linked_obj = PathBuf::from(format!("{}_linked.o", prefix));
    {
        let mut cmd = Command::new(&llc);
        cmd.arg("-filetype=obj")
            .arg("-O3")
            .arg(&opt_bc)
            .arg("-o")
            .arg(&linked_obj);
        if let Some(triple) = target_triple {
            cmd.arg("-mtriple").arg(triple);
        }
        log::debug!("perry-codegen llc: {:?}", cmd);
        let output = cmd.output().context("Failed to invoke llc")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!(
                "llc failed (status={}):\n{}",
                output.status,
                stderr
            ));
        }
    }

    if keep {
        eprintln!("[perry-codegen] bitcode-link intermediates kept:");
        for f in &intermediates {
            eprintln!("  {}", f.display());
        }
        eprintln!("  → {}", linked_obj.display());
    } else {
        for f in &intermediates {
            let _ = fs::remove_file(f);
        }
    }

    Ok(linked_obj)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version_block(target_line: &str) -> String {
        format!("clang version 18.0.0\n{}\nThread model: posix", target_line)
    }

    #[test]
    fn hint_for_mingw_clang_on_windows_targets_msvc() {
        // Only the host-is-windows arm fires this hint. The build matrix runs
        // these tests on every host, so we gate the assertion on cfg(windows).
        // On non-Windows hosts the function falls through to the generic
        // PERRY_LLVM_CLANG suggestion — also asserted below.
        let v = version_block("Target: x86_64-w64-windows-gnu");
        let hint = build_clang_failure_hint(
            "lld-link: error: undefined symbol: __main",
            &v,
            "x86_64-pc-windows-msvc",
        );
        if cfg!(target_os = "windows") {
            assert!(
                hint.contains("MinGW/GNU"),
                "expected MinGW hint, got: {}",
                hint
            );
            assert!(hint.contains("winget install LLVM.LLVM"));
            assert!(hint.contains("PERRY_LLVM_CLANG"));
        } else {
            assert!(hint.contains("PERRY_LLVM_CLANG"));
        }
    }

    #[test]
    fn hint_for_override_module_target_triple_warning() {
        let v = version_block("Target: x86_64-pc-linux-gnu");
        let hint = build_clang_failure_hint(
            "warning: overriding the module target triple with x86_64-pc-linux-gnu",
            &v,
            "x86_64-unknown-linux-gnu",
        );
        // On non-Windows hosts the override-warning branch should win.
        if !cfg!(target_os = "windows") {
            assert!(
                hint.contains("overriding the module target triple"),
                "expected override hint, got: {}",
                hint
            );
        }
    }

    #[test]
    fn hint_for_missing_library_message() {
        let v = version_block("Target: aarch64-apple-darwin23.0.0");
        let hint = build_clang_failure_hint(
            "ld: library not found for -lSystem",
            &v,
            "arm64-apple-macosx15.0.0",
        );
        assert!(
            hint.contains("library") || hint.contains("PERRY_LLVM_CLANG"),
            "got: {}",
            hint
        );
    }

    #[test]
    fn hint_falls_back_when_no_pattern_matches() {
        let v = version_block("Target: aarch64-apple-darwin23.0.0");
        let hint = build_clang_failure_hint(
            "(some unrelated clang stderr)",
            &v,
            "arm64-apple-macosx15.0.0",
        );
        assert!(
            hint.contains("PERRY_LLVM_CLANG"),
            "fallback hint should mention PERRY_LLVM_CLANG; got: {}",
            hint
        );
        assert!(hint.contains("arm64-apple-macosx15.0.0"));
    }
}
