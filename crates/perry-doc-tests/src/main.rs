//! Doc-example test harness.
//!
//! Discovers `.ts` files under `docs/examples/`, compiles each with `perry`,
//! runs the resulting binary (with `PERRY_UI_TEST_MODE=1` for UI examples),
//! and reports pass/fail. Optionally diffs stdout against `_expected/*.stdout`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use serde::Serialize;

mod image_diff;
mod lint;

#[derive(Parser, Debug)]
#[command(name = "doc-tests", about = "Perry documentation-example test harness")]
struct Cli {
    /// Only run examples whose relative path contains this substring.
    #[arg(long)]
    filter: Option<String>,

    /// Skip examples whose relative path contains this substring.
    #[arg(long)]
    filter_exclude: Option<String>,

    /// Write a JSON report to this path.
    #[arg(long)]
    json: Option<PathBuf>,

    /// Emit verbose per-example progress to stderr.
    #[arg(long, short)]
    verbose: bool,

    /// Override the perry binary to use (defaults to `./target/release/perry`).
    #[arg(long)]
    perry: Option<PathBuf>,

    /// Override the docs/examples root (defaults to `./docs/examples`).
    #[arg(long)]
    examples_dir: Option<PathBuf>,

    /// Skip compiling; rerun against binaries already in the out dir.
    #[arg(long)]
    no_compile: bool,

    /// Regenerate screenshot baselines for this host OS instead of diffing.
    #[arg(long)]
    bless: bool,

    /// Instead of running examples, scan the given markdown directory and
    /// report fenced `typescript` blocks that are neither `{{#include}}`
    /// directives nor annotated with `,no-test`.
    #[arg(long)]
    lint: Option<PathBuf>,

    /// Skip the host-platform run phase; only run the `targets:` cross-compile.
    #[arg(long)]
    xcompile_only: bool,

    /// Skip the `targets:` cross-compile phase entirely.
    #[arg(long)]
    skip_xcompile: bool,

    /// Restrict the cross-compile phase to just these targets (comma-separated).
    /// Each must still appear in an example's banner `targets:` list —
    /// this acts as an include-filter on top of the banner.
    #[arg(long, value_delimiter = ',')]
    xcompile_only_target: Vec<String>,
}

#[derive(Serialize, Debug, Clone)]
struct ExampleReport {
    file: String,
    kind: ExampleKind,
    status: Status,
    detail: String,
    duration_ms: u128,
}

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Status {
    Pass,
    CompileFail,
    RunFail,
    StdoutDiff,
    ScreenshotDiff,
    Timeout,
    Skip,
    Blessed,
    CrossCompilePass,
    CrossCompileFail,
    CrossCompileSkip,
}

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ExampleKind {
    Ui,
    Runtime,
}

#[derive(Serialize, Debug)]
struct FullReport {
    total: usize,
    passed: usize,
    failed: usize,
    skipped: usize,
    host_platform: &'static str,
    results: Vec<ExampleReport>,
}

fn main() {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("doc-tests: fatal: {e:#}");
            std::process::exit(2);
        }
    }
}

fn run(cli: &Cli) -> Result<i32> {
    let repo_root = find_repo_root()?;

    if let Some(lint_dir) = &cli.lint {
        return run_lint(lint_dir);
    }

    let examples_dir = cli
        .examples_dir
        .clone()
        .unwrap_or_else(|| repo_root.join("docs/examples"));
    let perry_bin = cli.perry.clone().unwrap_or_else(|| {
        repo_root.join(if cfg!(windows) {
            "target/release/perry.exe"
        } else {
            "target/release/perry"
        })
    });

    if !examples_dir.is_dir() {
        return Err(anyhow!(
            "examples directory not found: {}",
            examples_dir.display()
        ));
    }
    if !cli.no_compile && !perry_bin.is_file() {
        return Err(anyhow!(
            "perry binary not found at {} — run `cargo build --release -p perry` first",
            perry_bin.display()
        ));
    }

    let host = host_platform();
    let out_dir = repo_root.join("target/perry-doc-tests");
    std::fs::create_dir_all(&out_dir).ok();

    let examples = discover_examples(&examples_dir)?;
    let mut results = Vec::with_capacity(examples.len());

    for ex in examples {
        let rel = pathdiff(&examples_dir, &ex.path);
        if let Some(f) = &cli.filter {
            if !rel.contains(f) {
                continue;
            }
        }
        if let Some(f) = &cli.filter_exclude {
            if rel.contains(f) {
                continue;
            }
        }

        if !cli.xcompile_only {
            if !ex.platforms.contains(host) {
                results.push(ExampleReport {
                    file: rel.clone(),
                    kind: ex.kind,
                    status: Status::Skip,
                    detail: format!("platform `{host}` not listed in banner"),
                    duration_ms: 0,
                });
            } else {
                let started = Instant::now();
                if cli.verbose {
                    eprintln!("[run] {rel}");
                }
                let mut report = run_one(
                    &ex,
                    &rel,
                    &examples_dir,
                    &perry_bin,
                    &out_dir,
                    cli.no_compile,
                    host,
                    cli.bless,
                );
                report.duration_ms = started.elapsed().as_millis();
                if cli.verbose {
                    eprintln!("   -> {:?} ({} ms)", report.status, report.duration_ms);
                }
                results.push(report);
            }
        }

        if cli.skip_xcompile {
            continue;
        }

        // Cross-compile phase: for each extra target listed in the banner,
        // invoke `perry compile --target <t>` and check exit + artifact. No
        // execution — this catches drift in platform-specific code paths.
        for target in &ex.targets {
            if !cli.xcompile_only_target.is_empty()
                && !cli.xcompile_only_target.iter().any(|t| t == target)
            {
                continue;
            }
            if let Some(reason) = target_buildable_reason(target, host) {
                results.push(ExampleReport {
                    file: rel.clone(),
                    kind: ex.kind,
                    status: Status::CrossCompileSkip,
                    detail: format!("target=`{target}`: {reason}"),
                    duration_ms: 0,
                });
                continue;
            }
            let started = Instant::now();
            if cli.verbose {
                eprintln!("[xcompile] {rel} --target {target}");
            }
            let mut report =
                cross_compile_one(&ex, &rel, &perry_bin, &out_dir, target);
            report.duration_ms = started.elapsed().as_millis();
            if cli.verbose {
                eprintln!("   -> {:?} ({} ms)", report.status, report.duration_ms);
            }
            results.push(report);
        }
    }

    let (passed, failed, skipped) = count(&results);
    let total = results.len();
    print_summary(total, passed, failed, skipped, &results);

    if let Some(path) = &cli.json {
        let full = FullReport {
            total,
            passed,
            failed,
            skipped,
            host_platform: host,
            results: results.clone(),
        };
        let contents =
            serde_json::to_string_pretty(&full).context("serializing JSON report")?;
        std::fs::write(path, contents).context("writing JSON report")?;
    }

    Ok(if failed == 0 { 0 } else { 1 })
}

fn run_lint(dir: &Path) -> Result<i32> {
    let violations = lint::run(dir)?;
    if violations.is_empty() {
        println!("lint: {} ok — no untagged typescript fences", dir.display());
        return Ok(0);
    }
    println!(
        "lint: {} untagged typescript fence(s) in {}:",
        violations.len(),
        dir.display()
    );
    for v in &violations {
        println!("  {}:{} `{}`", v.file.display(), v.line, v.fence);
        if !v.first_body_line.is_empty() {
            println!("    | {}", v.first_body_line);
        }
    }
    println!();
    println!(
        "Each fence must either be a pure `{{{{#include ...}}}}` directive or declare \
         `typescript,no-test` to opt out of compile-testing.",
    );
    Ok(1)
}

fn count(results: &[ExampleReport]) -> (usize, usize, usize) {
    let mut passed = 0;
    let mut failed = 0;
    let mut skipped = 0;
    for r in results {
        match r.status {
            Status::Pass | Status::Blessed | Status::CrossCompilePass => passed += 1,
            Status::Skip | Status::CrossCompileSkip => skipped += 1,
            _ => failed += 1,
        }
    }
    (passed, failed, skipped)
}

fn print_summary(total: usize, passed: usize, failed: usize, skipped: usize, results: &[ExampleReport]) {
    for r in results {
        let tag = match r.status {
            Status::Pass => "PASS",
            Status::Blessed => "BLESSED",
            Status::CompileFail => "COMPILE_FAIL",
            Status::RunFail => "RUN_FAIL",
            Status::StdoutDiff => "STDOUT_DIFF",
            Status::ScreenshotDiff => "SCREENSHOT_DIFF",
            Status::Timeout => "TIMEOUT",
            Status::Skip => "SKIP",
            Status::CrossCompilePass => "XCOMPILE_PASS",
            Status::CrossCompileFail => "XCOMPILE_FAIL",
            Status::CrossCompileSkip => "XCOMPILE_SKIP",
        };
        println!("{tag:<13} {} ({} ms)  {}", r.file, r.duration_ms, r.detail);
    }
    println!();
    println!(
        "doc-tests: {passed}/{total} passed, {failed} failed, {skipped} skipped",
    );
}

#[derive(Debug)]
struct Example {
    path: PathBuf,
    kind: ExampleKind,
    platforms: BTreeSet<String>,
    targets: BTreeSet<String>,
    compile_only: bool,
}

fn discover_examples(root: &Path) -> Result<Vec<Example>> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("ts") {
            continue;
        }
        // Skip harness/support files.
        if path.components().any(|c| {
            matches!(
                c.as_os_str().to_str(),
                Some("_harness") | Some("_baselines") | Some("_expected") | Some("_reports")
            )
        }) {
            continue;
        }
        let banner = read_banner(path)?;
        let kind = if path.components().any(|c| c.as_os_str() == "ui") {
            ExampleKind::Ui
        } else {
            ExampleKind::Runtime
        };
        out.push(Example {
            path: path.to_path_buf(),
            kind,
            platforms: banner.platforms,
            targets: banner.targets,
            compile_only: banner.compile_only,
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

#[derive(Debug, Default)]
struct Banner {
    platforms: BTreeSet<String>,
    targets: BTreeSet<String>,
    /// When true, the harness compiles the example but doesn't run it.
    /// Useful for programs that bind ports, depend on external services,
    /// or otherwise can't be driven to a clean exit under the default
    /// single-program timeout. Catches API/TS drift without the
    /// integration-test overhead.
    compile_only: bool,
}

fn read_banner(path: &Path) -> Result<Banner> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;
    let mut b = Banner::default();
    for line in text.lines().take(15) {
        let line = line.trim_start();
        if !line.starts_with("//") {
            break;
        }
        let body = line.trim_start_matches("//").trim();
        if let Some(rest) = body.strip_prefix("platforms:") {
            for item in rest.split(',') {
                let t = item.trim();
                if !t.is_empty() {
                    b.platforms.insert(t.to_string());
                }
            }
        } else if let Some(rest) = body.strip_prefix("targets:") {
            for item in rest.split(',') {
                let t = item.trim();
                if !t.is_empty() {
                    b.targets.insert(t.to_string());
                }
            }
        } else if let Some(rest) = body.strip_prefix("run:") {
            let v = rest.trim();
            if v.eq_ignore_ascii_case("false") || v == "0" || v.eq_ignore_ascii_case("no") {
                b.compile_only = true;
            }
        }
    }
    // Default to "all hosts" if banner didn't specify.
    if b.platforms.is_empty() {
        for p in ["macos", "linux", "windows"] {
            b.platforms.insert(p.to_string());
        }
    }
    Ok(b)
}

fn run_one(
    ex: &Example,
    rel: &str,
    examples_dir: &Path,
    perry_bin: &Path,
    out_dir: &Path,
    no_compile: bool,
    host: &'static str,
    bless: bool,
) -> ExampleReport {
    let stem = safe_stem(rel);
    let bin_path = out_dir.join(if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.clone()
    });

    if !no_compile {
        if let Err(e) = compile(perry_bin, &ex.path, &bin_path) {
            return ExampleReport {
                file: rel.to_string(),
                kind: ex.kind,
                status: Status::CompileFail,
                detail: trim_detail(&e.to_string()),
                duration_ms: 0,
            };
        }
    }

    // `// run: false` — compile-only examples (servers that bind ports,
    // programs that need external services, etc.) just verify the TS
    // compiles and links cleanly. Drift protection for the API shape
    // without the integration-test overhead.
    if ex.compile_only {
        return ExampleReport {
            file: rel.to_string(),
            kind: ex.kind,
            status: Status::Pass,
            detail: "compile-only (`run: false` banner)".to_string(),
            duration_ms: 0,
        };
    }

    let expected_stdout = load_expected_stdout(examples_dir, rel);
    let baseline_name = baseline_name_for(rel);
    let screenshot_path = baseline_name.as_ref().map(|name| {
        out_dir.join(format!("{name}_{host}.png"))
    });

    let mut cmd = Command::new(&bin_path);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    match ex.kind {
        ExampleKind::Ui => {
            cmd.env("PERRY_UI_TEST_MODE", "1");
            cmd.env("PERRY_UI_TEST_EXIT_AFTER_MS", "500");
            if let Some(sp) = &screenshot_path {
                cmd.env("PERRY_UI_SCREENSHOT_PATH", sp);
            }
        }
        ExampleKind::Runtime => {}
    }

    let timeout = match ex.kind {
        ExampleKind::Ui => Duration::from_secs(15),
        ExampleKind::Runtime => Duration::from_secs(10),
    };

    match run_with_timeout(&mut cmd, timeout) {
        Ok(out) => {
            if !out.status.success() {
                return ExampleReport {
                    file: rel.to_string(),
                    kind: ex.kind,
                    status: Status::RunFail,
                    detail: format!(
                        "exit={} {}",
                        out.status.code().unwrap_or(-1),
                        combine_stdio(&out.stdout, &out.stderr),
                    ),
                    duration_ms: 0,
                };
            }
            if let Some(expected) = expected_stdout {
                let actual = String::from_utf8_lossy(&out.stdout).to_string();
                if normalize(&actual) != normalize(&expected) {
                    return ExampleReport {
                        file: rel.to_string(),
                        kind: ex.kind,
                        status: Status::StdoutDiff,
                        detail: stdout_diff_summary(&expected, &actual),
                        duration_ms: 0,
                    };
                }
            }

            // Screenshot diff for the widget gallery (Phase 2).
            if let (Some(name), Some(sp)) = (&baseline_name, &screenshot_path) {
                return compare_or_bless_screenshot(
                    rel,
                    ex.kind,
                    examples_dir,
                    name,
                    sp,
                    host,
                    bless,
                );
            }

            ExampleReport {
                file: rel.to_string(),
                kind: ex.kind,
                status: Status::Pass,
                detail: String::new(),
                duration_ms: 0,
            }
        }
        Err(RunError::Timeout) => ExampleReport {
            file: rel.to_string(),
            kind: ex.kind,
            status: Status::Timeout,
            detail: format!("exceeded {:?}", timeout),
            duration_ms: 0,
        },
        Err(RunError::Io(e)) => ExampleReport {
            file: rel.to_string(),
            kind: ex.kind,
            status: Status::RunFail,
            detail: trim_detail(&e.to_string()),
            duration_ms: 0,
        },
    }
}

/// Whether a given `--target` value can be built from this host. Any
/// other combination gets reported as `XCOMPILE_SKIP` with the reason so
/// coverage stays visible without failing the job. Also checks whether
/// the host has the toolchain installed (Xcode, Android NDK) so that
/// local dev boxes don't hit false failures when they're simply missing
/// the mobile SDKs.
fn target_buildable_on_host(target: &str, host: &str) -> bool {
    target_buildable_reason(target, host).is_none()
}

/// Reason the target can't be built from this host, or None if it can.
fn target_buildable_reason(target: &str, host: &str) -> Option<String> {
    match target {
        "watchos" | "watchos-simulator" | "visionos" | "visionos-simulator" => {
            // Rust Tier-3 — requires nightly + `-Zbuild-std`. Skip in the
            // generic cross-compile until we wire up a dedicated job.
            Some(format!("target=`{target}` is Rust Tier-3 (nightly + -Zbuild-std); not yet wired"))
        }
        "ios" | "ios-simulator" | "tvos" | "tvos-simulator" | "macos"
        | "ios-widget" | "ios-widget-simulator" => {
            if host != "macos" {
                Some(format!("needs Xcode — host `{host}` can't build `{target}`"))
            } else if !has_xcode() {
                Some(format!("Xcode command-line tools not installed (target=`{target}`)"))
            } else {
                None
            }
        }
        "android" => {
            if host != "linux" && host != "macos" {
                Some(format!("android cross-compile unsupported on host `{host}`"))
            } else if std::env::var("ANDROID_NDK_HOME").is_err()
                && std::env::var("ANDROID_NDK_ROOT").is_err()
            {
                Some("ANDROID_NDK_HOME/ROOT not set".to_string())
            } else {
                None
            }
        }
        "linux" if host != "linux" => {
            Some(format!("linux cross-compile unsupported on host `{host}`"))
        }
        "windows" if host != "windows" => {
            Some(format!("windows cross-compile unsupported on host `{host}`"))
        }
        "linux" | "windows" | "web" | "wasm" => None,
        other => Some(format!("unknown target `{other}`")),
    }
}

fn has_xcode() -> bool {
    Command::new("xcrun")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn cross_compile_one(
    ex: &Example,
    rel: &str,
    perry_bin: &Path,
    out_dir: &Path,
    target: &str,
) -> ExampleReport {
    let stem = safe_stem(rel);
    // Output-path extension: perry itself creates the .app bundle directory
    // from a plain executable path (see crates/perry/src/commands/compile.rs
    // where `exe_path.with_extension("app")` sets up the bundle). If we
    // pass `foo.app` as -o, perry links a FILE at `foo.app`, then tries to
    // mkdir at the same path and copy the file into it — ENOTDIR.
    let ext = match target {
        "web" => ".html",
        "wasm" => ".wasm",
        "windows" => ".exe",
        "android" => ".apk",
        _ => "",
    };
    let out = out_dir.join(format!("{stem}__{target}{ext}"));
    // For Apple bundle targets, perry produces `<out>.app/` alongside (or
    // replacing) the linked binary; check that directory for the artifact.
    let artifact_check = match target {
        "ios" | "ios-simulator" | "visionos" | "visionos-simulator" | "tvos" | "tvos-simulator" | "watchos"
        | "watchos-simulator" | "ios-widget" | "ios-widget-simulator" => {
            out.with_extension("app")
        }
        _ => out.clone(),
    };

    let output = match Command::new(perry_bin)
        .arg("compile")
        .arg(&ex.path)
        .arg("--target")
        .arg(target)
        .arg("-o")
        .arg(&out)
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            return ExampleReport {
                file: rel.to_string(),
                kind: ex.kind,
                status: Status::CrossCompileFail,
                detail: format!("target=`{target}`: spawn failed: {e}"),
                duration_ms: 0,
            };
        }
    };

    if !output.status.success() {
        return ExampleReport {
            file: rel.to_string(),
            kind: ex.kind,
            status: Status::CrossCompileFail,
            detail: format!(
                "target=`{target}`: perry exit {}: {}",
                output.status.code().unwrap_or(-1),
                combine_stdio(&output.stdout, &output.stderr),
            ),
            duration_ms: 0,
        };
    }

    // Verify the artifact landed and has bytes. Apple .app targets produce
    // a directory bundle rather than a single file.
    let ok = if artifact_check.is_dir() {
        std::fs::read_dir(&artifact_check).map(|it| it.count() > 0).unwrap_or(false)
    } else {
        artifact_check.metadata().map(|m| m.len() > 0).unwrap_or(false)
    };
    if !ok {
        return ExampleReport {
            file: rel.to_string(),
            kind: ex.kind,
            status: Status::CrossCompileFail,
            detail: format!("target=`{target}`: artifact missing or empty at {}", artifact_check.display()),
            duration_ms: 0,
        };
    }

    ExampleReport {
        file: rel.to_string(),
        kind: ex.kind,
        status: Status::CrossCompilePass,
        detail: format!("target=`{target}`"),
        duration_ms: 0,
    }
}

fn compile(perry_bin: &Path, src: &Path, out: &Path) -> Result<()> {
    let out_status = Command::new(perry_bin)
        .arg(src)
        .arg("-o")
        .arg(out)
        .output()
        .with_context(|| format!("launching perry for {}", src.display()))?;
    if !out_status.status.success() {
        return Err(anyhow!(
            "perry exit {}: {}",
            out_status.status.code().unwrap_or(-1),
            combine_stdio(&out_status.stdout, &out_status.stderr),
        ));
    }
    Ok(())
}

/// Merge child stdout + stderr for error reporting. MSVC `link.exe` on
/// Windows writes LNKxxxx errors to stdout, not stderr, so failing to
/// include both hid the real linker diagnostic behind a generic perry
/// "Linking failed" message.
fn combine_stdio(stdout: &[u8], stderr: &[u8]) -> String {
    let o = String::from_utf8_lossy(stdout);
    let e = String::from_utf8_lossy(stderr);
    let merged = match (o.trim().is_empty(), e.trim().is_empty()) {
        (true, _) => e.to_string(),
        (false, true) => o.to_string(),
        (false, false) => format!("{e}\n--- stdout ---\n{o}"),
    };
    trim_detail(&merged)
}

enum RunError {
    Timeout,
    Io(std::io::Error),
}

struct RunOutput {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn run_with_timeout(cmd: &mut Command, timeout: Duration) -> std::result::Result<RunOutput, RunError> {
    let mut child = cmd.spawn().map_err(RunError::Io)?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = Vec::new();
                let mut stderr = Vec::new();
                if let Some(mut s) = child.stdout.take() {
                    let _ = std::io::Read::read_to_end(&mut s, &mut stdout);
                }
                if let Some(mut s) = child.stderr.take() {
                    let _ = std::io::Read::read_to_end(&mut s, &mut stderr);
                }
                return Ok(RunOutput { status, stdout, stderr });
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(RunError::Timeout);
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(e) => return Err(RunError::Io(e)),
        }
    }
}

/// The screenshot-tested examples. Only files listed here get their output
/// diffed against a checked-in baseline — every other UI example just needs
/// to launch + exit cleanly (Phase 1).
fn baseline_name_for(rel: &str) -> Option<String> {
    match rel {
        "ui/gallery.ts" => Some("gallery".to_string()),
        _ => None,
    }
}

fn compare_or_bless_screenshot(
    rel: &str,
    kind: ExampleKind,
    examples_dir: &Path,
    baseline_name: &str,
    screenshot_path: &Path,
    host: &'static str,
    bless: bool,
) -> ExampleReport {
    let baseline_dir = examples_dir.join("_baselines").join(host);
    let baseline_path = baseline_dir.join(format!("{baseline_name}.png"));

    if !screenshot_path.is_file() {
        return ExampleReport {
            file: rel.to_string(),
            kind,
            status: Status::ScreenshotDiff,
            detail: format!(
                "no screenshot captured at {} — backend may not honor PERRY_UI_SCREENSHOT_PATH on this platform",
                screenshot_path.display()
            ),
            duration_ms: 0,
        };
    }

    if bless {
        if let Err(e) = std::fs::create_dir_all(&baseline_dir) {
            return ExampleReport {
                file: rel.to_string(),
                kind,
                status: Status::ScreenshotDiff,
                detail: format!("creating baseline dir: {e}"),
                duration_ms: 0,
            };
        }
        if let Err(e) = std::fs::copy(screenshot_path, &baseline_path) {
            return ExampleReport {
                file: rel.to_string(),
                kind,
                status: Status::ScreenshotDiff,
                detail: format!("writing baseline: {e}"),
                duration_ms: 0,
            };
        }
        return ExampleReport {
            file: rel.to_string(),
            kind,
            status: Status::Blessed,
            detail: format!("baseline written to {}", baseline_path.display()),
            duration_ms: 0,
        };
    }

    if !baseline_path.is_file() {
        return ExampleReport {
            file: rel.to_string(),
            kind,
            status: Status::ScreenshotDiff,
            detail: format!(
                "no baseline at {} — run with --bless to create one",
                baseline_path.display()
            ),
            duration_ms: 0,
        };
    }

    let thresholds_file = examples_dir.join("_baselines/thresholds.json");
    let threshold = image_diff::threshold_for(&thresholds_file, baseline_name, host);

    match image_diff::diff(screenshot_path, &baseline_path, threshold) {
        Ok(outcome) if outcome.passed() => ExampleReport {
            file: rel.to_string(),
            kind,
            status: Status::Pass,
            detail: format!("dssim={:.5} <= {:.5}", outcome.distance, outcome.threshold),
            duration_ms: 0,
        },
        Ok(outcome) => ExampleReport {
            file: rel.to_string(),
            kind,
            status: Status::ScreenshotDiff,
            detail: format!(
                "dssim={:.5} > {:.5} (actual={} baseline={})",
                outcome.distance,
                outcome.threshold,
                screenshot_path.display(),
                baseline_path.display(),
            ),
            duration_ms: 0,
        },
        Err(e) => ExampleReport {
            file: rel.to_string(),
            kind,
            status: Status::ScreenshotDiff,
            detail: format!("dssim error: {e}"),
            duration_ms: 0,
        },
    }
}

fn load_expected_stdout(examples_dir: &Path, rel: &str) -> Option<String> {
    let candidate = examples_dir
        .join("_expected")
        .join(format!("{}.stdout", rel.trim_end_matches(".ts")));
    std::fs::read_to_string(candidate).ok()
}

fn normalize(s: &str) -> String {
    s.replace("\r\n", "\n").trim_end().to_string()
}

fn stdout_diff_summary(expected: &str, actual: &str) -> String {
    let e_lines: Vec<&str> = expected.lines().collect();
    let a_lines: Vec<&str> = actual.lines().collect();
    let common = e_lines.iter().zip(a_lines.iter()).take_while(|(a, b)| a == b).count();
    let first_diff_line = common + 1;
    let e_snippet = e_lines.get(common).unwrap_or(&"");
    let a_snippet = a_lines.get(common).unwrap_or(&"");
    format!(
        "first diff at line {first_diff_line}: expected={:?} actual={:?}",
        trim_detail(e_snippet),
        trim_detail(a_snippet)
    )
}

fn trim_detail(s: &str) -> String {
    let s = s.trim();
    if s.len() > 4000 {
        // Keep both ends — head has "Compiling libc v0.2.184..." noise but
        // the real error usually sits at the tail.
        let head = &s[..1000];
        let tail = &s[s.len() - 2500..];
        format!("{head}\n  ... [truncated] ...\n{tail}")
    } else {
        s.to_string()
    }
}

fn safe_stem(rel: &str) -> String {
    rel.trim_end_matches(".ts")
        .replace(['/', '\\', ' ', '.'], "_")
}

fn pathdiff(base: &Path, child: &Path) -> String {
    child
        .strip_prefix(base)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| child.to_string_lossy().to_string())
}

fn host_platform() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "macos"
    }
    #[cfg(target_os = "linux")]
    {
        "linux"
    }
    #[cfg(target_os = "windows")]
    {
        "windows"
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        "unknown"
    }
}

fn find_repo_root() -> Result<PathBuf> {
    let cwd = std::env::current_dir()?;
    let mut cur: &Path = &cwd;
    loop {
        if cur.join("Cargo.toml").is_file() && cur.join("crates").is_dir() {
            return Ok(cur.to_path_buf());
        }
        match cur.parent() {
            Some(p) => cur = p,
            None => {
                return Err(anyhow!(
                    "could not find repo root (no Cargo.toml with crates/ above {})",
                    cwd.display()
                ))
            }
        }
    }
}
