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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use anyhow::{anyhow, Context, Result};

/// Cached result of the pre-flight clang probe — evaluated once per process.
/// `Some(default_triple)` if the probe succeeded, `None` if it failed.
static CLANG_PROBE: OnceLock<Option<String>> = OnceLock::new();

/// Strictly-monotonic per-process counter mixed into temp .ll/.o paths so two
/// rayon codegen workers calling `compile_ll_to_object` concurrently can never
/// land on the same path. SystemTime::now().as_nanos() alone isn't enough —
/// macOS clocks resolve to microseconds, and rayon happily schedules sibling
/// modules within the same microsecond, producing identical paths. When that
/// happens, both workers overwrite the same .ll, both invoke clang on it, and
/// both read back identical bytes — leaving sibling .o files with one
/// module's symbols stamped onto the other's filename. (Closes #509.)
static TEMP_NONCE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
struct ClangCompilePlan {
    clang: PathBuf,
    effective_target: String,
    clang_args: Vec<String>,
    analysis_clang_args: Vec<String>,
    native_tuning_arg: Option<String>,
    ll_path: PathBuf,
    obj_path: PathBuf,
    stderr_remarks_path: PathBuf,
}

fn native_tuning_arg_for_host() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "-mcpu=native"
    } else {
        "-march=native"
    }
}

/// Default IR-size cutoff above which a module is compiled at `-O0` instead
/// of `-O3` (#4880). A module dominated by a huge generated literal
/// (config / lookup table) lowers to one enormous function whose
/// thousands of `alloca`s make LLVM's `-O1+` pipeline (SROA / mem2reg /
/// GVN) super-linear: a 2800-key object literal is ~10 MB of IR that
/// `clang -c -O3` chews on for ~18 s (and multi-thousand-key literals were
/// reported taking minutes / getting killed), versus ~3 s at `-O0`.
/// `-O1`/`-O2` are no faster than `-O3` here, so `-O0` is the only escape.
/// Such modules are almost always static data where optimization is
/// irrelevant. Tunable via `PERRY_LL_O0_THRESHOLD_BYTES`.
const DEFAULT_LL_O0_THRESHOLD_BYTES: usize = 6 * 1024 * 1024;

fn ll_o0_threshold_bytes() -> usize {
    std::env::var("PERRY_LL_O0_THRESHOLD_BYTES")
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok())
        .unwrap_or(DEFAULT_LL_O0_THRESHOLD_BYTES)
}

/// For an oversized unit (one past `PERRY_LL_O0_THRESHOLD_BYTES`), the average
/// IR bytes-per-function below which we size-optimize at `-Os` instead of
/// dropping to `-O0`.
///
/// `-O0` exists purely to dodge the `#4880` pathology: a unit dominated by ONE
/// enormous generated function (a multi-thousand-element data literal lowering
/// to a single 800k-line function) makes LLVM's `-O1+`/`-Os` pipeline
/// super-linear and effectively never finishes. But that pathology is *not* the
/// common oversized case — a large minified bundle is tens of thousands of
/// ordinary functions, none individually huge. Those compile fine at `-Os`,
/// which emits ~30-50% less `__text` than `-O0` (no register-pressure-blind
/// spilling, dead code folded) for only a ~2-3x clang-time cost that is well
/// amortized across the bundle.
///
/// Average bytes-per-function cleanly separates the two: a pathological monolith
/// is megabytes-per-function (the `#4880` 400k-element literal is ~9 MB/fn),
/// whereas real bundles are ~20 KB/fn — a >100x gap. Staying conservative we
/// keep `-O0` for any unit averaging above this cap, so a giant-literal unit
/// (always few, very large functions) never reaches the `-Os` pipeline.
/// Tunable via `PERRY_LL_SIZE_OPT_MAX_FN_BYTES`; `PERRY_LL_SIZE_OPT=0`/`off`
/// forces the old `-O0` behavior, `=1`/`on` forces `-Os` regardless of density.
const DEFAULT_LL_SIZE_OPT_MAX_FN_BYTES: usize = 256 * 1024;

fn ll_size_opt_max_fn_bytes() -> usize {
    std::env::var("PERRY_LL_SIZE_OPT_MAX_FN_BYTES")
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok())
        .unwrap_or(DEFAULT_LL_SIZE_OPT_MAX_FN_BYTES)
}

/// Decide the clang opt flag for an oversized unit: `-Os` when the unit is many
/// ordinary functions (size-optimize, big `__text` win), `-O0` when it is a
/// pathological few-giant-function monolith (`#4880`). `ll_fn_count` is the
/// number of `define` functions in the unit.
fn oversized_opt_flag(ll_byte_size: usize, ll_fn_count: usize) -> &'static str {
    match std::env::var("PERRY_LL_SIZE_OPT").as_deref() {
        Ok("0") | Ok("off") | Ok("false") => return "-O0",
        Ok("1") | Ok("on") | Ok("true") => return "-Os",
        _ => {}
    }
    let avg_fn_bytes = ll_byte_size / ll_fn_count.max(1);
    if avg_fn_bytes <= ll_size_opt_max_fn_bytes() {
        "-Os"
    } else {
        "-O0"
    }
}

/// Count `define` functions in an LLVM IR text unit. Cheap single scan; every
/// function definition begins a line `define …`, always preceded by a newline
/// (module header/target-triple lines come first).
fn count_ll_functions(ll_text: &str) -> usize {
    ll_text.match_indices("\ndefine ").count()
}

fn build_clang_compile_plan(
    clang: PathBuf,
    ll_path: PathBuf,
    obj_path: PathBuf,
    target_triple: Option<&str>,
    ll_byte_size: usize,
    ll_fn_count: usize,
) -> ClangCompilePlan {
    let effective_target = target_triple
        .map(|s| s.to_string())
        .unwrap_or_else(crate::codegen::default_target_triple);
    let native_tuning_arg = target_triple
        .is_none()
        .then(|| native_tuning_arg_for_host().to_string());
    let stderr_remarks_path = PathBuf::from(format!("{}.clang-stderr", obj_path.display()));

    // #4880: oversized modules don't get the speed-tuned -O3 pipeline (it goes
    // super-linear on giant generated functions). Instead size-optimize at -Os
    // — which emits far less __text than -O0 — UNLESS the unit is a pathological
    // few-giant-function monolith (giant data literal), in which case only -O0
    // finishes in practical time. See DEFAULT_LL_O0_THRESHOLD_BYTES /
    // oversized_opt_flag.
    let o0_threshold = ll_o0_threshold_bytes();
    let opt_flag = if o0_threshold > 0 && ll_byte_size > o0_threshold {
        let flag = oversized_opt_flag(ll_byte_size, ll_fn_count);
        eprintln!(
            "perry: module IR is {:.1} MB (> {:.1} MB), {} functions \
             (~{:.0} KB/fn); compiling at {} instead of -O3 so LLVM's -O1+ \
             pipeline doesn't blow up on oversized functions (#4880). Override \
             with PERRY_LL_O0_THRESHOLD_BYTES / PERRY_LL_SIZE_OPT.",
            ll_byte_size as f64 / (1024.0 * 1024.0),
            o0_threshold as f64 / (1024.0 * 1024.0),
            ll_fn_count,
            (ll_byte_size as f64 / ll_fn_count.max(1) as f64) / 1024.0,
            flag,
        );
        flag
    } else {
        "-O3"
    };

    let mut clang_args = vec!["-c".to_string(), opt_flag.to_string()];
    if std::env::var("PERRY_DEBUG_SYMBOLS").is_ok() {
        clang_args.push("-g".to_string());
    }
    clang_args.push("-fno-math-errno".to_string());
    if let Some(arg) = &native_tuning_arg {
        clang_args.push(arg.clone());
    }
    clang_args.push(ll_path.display().to_string());
    clang_args.push("-o".to_string());
    clang_args.push(obj_path.display().to_string());
    clang_args.push("-target".to_string());
    clang_args.push(effective_target.clone());

    let mut analysis_clang_args = vec!["-O3".to_string(), "-fno-math-errno".to_string()];
    if let Some(arg) = &native_tuning_arg {
        analysis_clang_args.push(arg.clone());
    }
    analysis_clang_args.push("-target".to_string());
    analysis_clang_args.push(effective_target.clone());

    ClangCompilePlan {
        clang,
        effective_target,
        clang_args,
        analysis_clang_args,
        native_tuning_arg,
        ll_path,
        obj_path,
        stderr_remarks_path,
    }
}

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
    // Per-call unique counter — strictly monotonic, no collisions across
    // rayon workers in the same process. We still mix in the wall-clock
    // nanos for cross-process distinctness (two `perry` invocations can
    // share /tmp), but the counter is what guarantees in-process safety.
    let counter = TEMP_NONCE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let wall_nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let ll_path = tmp_dir.join(format!("perry_llvm_{}_{}_{}.ll", pid, wall_nonce, counter));
    let obj_path = tmp_dir.join(format!("perry_llvm_{}_{}_{}.o", pid, wall_nonce, counter));

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

    let plan = build_clang_compile_plan(
        clang.clone(),
        ll_path.clone(),
        obj_path.clone(),
        target_triple,
        ll_text.len(),
        count_ll_functions(ll_text),
    );

    // Pre-flight probe: capture clang's default Target: line once per process,
    // so we can warn early if it disagrees with the IR's triple in a way that
    // historically broke Windows builds. The actual build still succeeds via
    // the explicit -target pin below — the probe is purely informational.
    probe_clang_default_triple(&plan.clang, &plan.effective_target);

    let mut cmd = Command::new(&plan.clang);
    cmd.args(&plan.clang_args);
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
    //
    // Native CPU tuning remains part of the same plan when no explicit target
    // is supplied: only host builds receive `-mcpu=native` / `-march=native`.

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
        let hint = build_clang_failure_hint(&stderr, &clang_version, &plan.effective_target);
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
            plan.clang.display(),
            clang_version.lines().next().unwrap_or("?"),
            plan.effective_target,
            plan.ll_path.display(),
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
        let _ = fs::write(&plan.stderr_remarks_path, &output.stderr);
        let metadata_path = PathBuf::from(format!("{}.compile-plan.json", plan.obj_path.display()));
        write_compile_plan_metadata(&plan, &metadata_path)?;
        eprintln!("[perry-codegen] kept LLVM IR: {}", plan.ll_path.display());
        eprintln!("[perry-codegen] kept object:  {}", plan.obj_path.display());
        eprintln!(
            "[perry-codegen] kept compile metadata: {}",
            metadata_path.display()
        );
    } else {
        let _ = fs::remove_file(&plan.ll_path);
        let _ = fs::remove_file(&plan.obj_path);
    }

    Ok(bytes)
}

/// Compile a module that was split into codegen units (#5391) to a SINGLE
/// object file's bytes. Each unit `.ll` (from `LlModule::render_codegen_units`)
/// is compiled independently by `clang -c` — bounding peak compiler memory to
/// roughly one unit's worth instead of the whole module — and the resulting
/// objects are merged with a partial link (`ld -r`) into one object, preserving
/// `compile_module`'s single-`Vec<u8>` contract and the existing one-object
/// link path. Units are compiled sequentially so peak RSS stays at one unit.
pub fn compile_units_to_object(units: &[String], target_triple: Option<&str>) -> Result<Vec<u8>> {
    match units {
        [] => return compile_ll_to_object("", target_triple),
        [only] => return compile_ll_to_object(only, target_triple),
        _ => {}
    }

    let tmp_dir = env::temp_dir();
    let pid = std::process::id();
    let nonce = TEMP_NONCE_COUNTER.fetch_add(1, Ordering::Relaxed);

    let mut obj_paths: Vec<PathBuf> = Vec::with_capacity(units.len());
    for (i, unit) in units.iter().enumerate() {
        let bytes = compile_ll_to_object(unit, target_triple)
            .with_context(|| format!("codegen unit {}/{} failed to compile", i + 1, units.len()))?;
        let p = tmp_dir.join(format!("perry_cgu_{}_{}_{}.o", pid, nonce, i));
        fs::write(&p, &bytes)
            .with_context(|| format!("failed to write codegen-unit object {}", p.display()))?;
        obj_paths.push(p);
    }

    let combined = tmp_dir.join(format!("perry_cgu_{}_{}_combined.o", pid, nonce));
    let ld = env::var("PERRY_LD").unwrap_or_else(|_| "ld".to_string());
    let mut cmd = Command::new(&ld);
    cmd.arg("-r").arg("-o").arg(&combined);
    for p in &obj_paths {
        cmd.arg(p);
    }
    let out = cmd
        .output()
        .with_context(|| format!("failed to invoke partial linker `{} -r`", ld))?;
    let result = if out.status.success() {
        fs::read(&combined)
            .with_context(|| format!("failed to read merged object {}", combined.display()))
    } else {
        Err(anyhow!(
            "partial link `{} -r` of {} codegen units failed (status={}).\nstderr:\n{}",
            ld,
            units.len(),
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ))
    };

    if env::var_os("PERRY_LLVM_KEEP_IR").is_none() {
        for p in &obj_paths {
            let _ = fs::remove_file(p);
        }
        let _ = fs::remove_file(&combined);
    }
    result
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn json_string_array(values: &[String]) -> String {
    let mut out = String::from("[");
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            out.push_str(", ");
        }
        out.push_str(&json_string(value));
    }
    out.push(']');
    out
}

fn json_optional_string(value: Option<&str>) -> String {
    value.map(json_string).unwrap_or_else(|| "null".to_string())
}

fn write_compile_plan_metadata(plan: &ClangCompilePlan, path: &Path) -> Result<()> {
    let text = format!(
        concat!(
            "{{\n",
            "  \"schema_version\": 1,\n",
            "  \"clang_path\": {},\n",
            "  \"effective_target\": {},\n",
            "  \"clang_args\": {},\n",
            "  \"analysis_clang_args\": {},\n",
            "  \"native_tuning_arg\": {},\n",
            "  \"llvm_ir_path\": {},\n",
            "  \"object_path\": {},\n",
            "  \"stderr_remarks_path\": {}\n",
            "}}\n"
        ),
        json_string(&plan.clang.display().to_string()),
        json_string(&plan.effective_target),
        json_string_array(&plan.clang_args),
        json_string_array(&plan.analysis_clang_args),
        json_optional_string(plan.native_tuning_arg.as_deref()),
        json_string(&plan.ll_path.display().to_string()),
        json_string(&plan.obj_path.display().to_string()),
        json_string(&plan.stderr_remarks_path.display().to_string()),
    );
    fs::write(path, text).with_context(|| {
        format!(
            "Failed to write compile-plan metadata at {}",
            path.display()
        )
    })
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

    #[test]
    fn compile_plan_records_effective_target_and_native_tuning() {
        let plan = build_clang_compile_plan(
            PathBuf::from("clang"),
            PathBuf::from("/tmp/input.ll"),
            PathBuf::from("/tmp/output.o"),
            None,
            0,
            0,
        );
        assert!(plan.clang_args.contains(&"-fno-math-errno".to_string()));
        // Small module → optimized at -O3 (#4880).
        assert!(plan.clang_args.contains(&"-O3".to_string()));
        assert!(plan.clang_args.contains(&"-target".to_string()));
        assert!(plan.analysis_clang_args.contains(&"-target".to_string()));
        assert_eq!(
            plan.native_tuning_arg.as_deref(),
            Some(native_tuning_arg_for_host())
        );
        assert!(!plan.effective_target.is_empty());
    }

    #[test]
    fn compile_plan_size_optimizes_oversized_many_function_module() {
        // An oversized unit made of many ordinary functions (a large minified
        // bundle: low bytes-per-function) size-optimizes at -Os — far less
        // __text than -O0 — rather than dropping to the speed pipeline or -O0.
        let huge = ll_o0_threshold_bytes() + 1;
        let many_funcs = huge / 1024; // ~1 KB/fn, well under the density cap
        let plan = build_clang_compile_plan(
            PathBuf::from("clang"),
            PathBuf::from("/tmp/input.ll"),
            PathBuf::from("/tmp/output.o"),
            None,
            huge,
            many_funcs,
        );
        assert!(plan.clang_args.contains(&"-Os".to_string()));
        assert!(!plan.clang_args.contains(&"-O3".to_string()));
        assert!(!plan.clang_args.contains(&"-O0".to_string()));
    }

    #[test]
    fn compile_plan_keeps_o0_for_oversized_giant_function_monolith() {
        // #4880: an oversized unit dominated by a few giant generated functions
        // (a multi-thousand-element data literal: megabytes-per-function) keeps
        // -O0, the only opt level whose pipeline finishes in practical time.
        let huge = ll_o0_threshold_bytes() + 1;
        let plan = build_clang_compile_plan(
            PathBuf::from("clang"),
            PathBuf::from("/tmp/input.ll"),
            PathBuf::from("/tmp/output.o"),
            None,
            huge,
            2, // ~3 MB/fn — far above the density cap
        );
        assert!(plan.clang_args.contains(&"-O0".to_string()));
        assert!(!plan.clang_args.contains(&"-O3".to_string()));
        assert!(!plan.clang_args.contains(&"-Os".to_string()));
    }

    #[test]
    fn compile_plan_skips_native_tuning_for_explicit_target() {
        let plan = build_clang_compile_plan(
            PathBuf::from("clang"),
            PathBuf::from("/tmp/input.ll"),
            PathBuf::from("/tmp/output.o"),
            Some("x86_64-unknown-linux-gnu"),
            0,
            0,
        );
        assert_eq!(plan.effective_target, "x86_64-unknown-linux-gnu");
        assert_eq!(plan.native_tuning_arg, None);
        assert!(!plan
            .clang_args
            .iter()
            .any(|arg| arg == "-march=native" || arg == "-mcpu=native"));
    }

    #[test]
    fn compile_plan_metadata_json_contains_object_source() {
        let temp = env::temp_dir().join(format!(
            "perry_compile_plan_test_{}_{}.json",
            std::process::id(),
            TEMP_NONCE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let plan = build_clang_compile_plan(
            PathBuf::from("clang"),
            PathBuf::from("/tmp/input.ll"),
            PathBuf::from("/tmp/output.o"),
            Some("x86_64-unknown-linux-gnu"),
            0,
            0,
        );
        write_compile_plan_metadata(&plan, &temp).unwrap();
        let text = fs::read_to_string(&temp).unwrap();
        let _ = fs::remove_file(&temp);
        assert!(text.contains("\"clang_path\": \"clang\""));
        assert!(text.contains("\"effective_target\": \"x86_64-unknown-linux-gnu\""));
        assert!(text.contains("\"object_path\": \"/tmp/output.o\""));
        assert!(text.contains("\"stderr_remarks_path\": \"/tmp/output.o.clang-stderr\""));
    }

    #[test]
    fn temp_nonce_counter_is_unique_across_concurrent_calls() {
        // Regression test for #509: two rayon workers calling
        // `compile_ll_to_object` concurrently must NOT generate the same
        // temp-file path. Pre-fix, the path was `perry_llvm_<pid>_<nanos>`
        // where `nanos` came from `SystemTime::now().as_nanos()`. On macOS
        // that resolves to microseconds, so two threads racing the path
        // construction in the same microsecond produced identical paths,
        // both overwrote the same .ll, and both clang invocations compiled
        // the same IR — leaving sibling .o files with identical bytes.
        //
        // The fix mixes `TEMP_NONCE_COUNTER.fetch_add(1, Relaxed)` into
        // the path. We verify here that 256 concurrent fetches produce
        // 256 distinct values, regardless of clock resolution.
        use std::collections::HashSet;
        use std::thread;

        let mut handles = Vec::new();
        for _ in 0..16 {
            handles.push(thread::spawn(|| {
                let mut local: Vec<u64> = Vec::with_capacity(16);
                for _ in 0..16 {
                    local.push(TEMP_NONCE_COUNTER.fetch_add(1, Ordering::Relaxed));
                }
                local
            }));
        }
        let mut all: Vec<u64> = Vec::with_capacity(256);
        for h in handles {
            all.extend(h.join().unwrap());
        }
        let unique: HashSet<u64> = all.iter().copied().collect();
        assert_eq!(
            unique.len(),
            all.len(),
            "TEMP_NONCE_COUNTER produced duplicate values: total={}, unique={}",
            all.len(),
            unique.len(),
        );
    }
}
