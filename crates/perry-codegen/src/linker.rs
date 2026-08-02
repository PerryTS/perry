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

use anyhow::{anyhow, bail, Context, Result};

/// Cached result of the pre-flight clang probe — evaluated once per process.
/// `Some(default_triple)` if the probe succeeded, `None` if it failed.
static CLANG_PROBE: OnceLock<Option<String>> = OnceLock::new();

/// Strictly-monotonic per-process counter mixed into **output** temp paths
/// (`.o`, multi-unit partial-link staging, etc.) so two rayon codegen workers
/// can never clobber each other's objects. (Closes #509.)
///
/// The **input** `.ll` basename is deliberately **not** mixed with this
/// counter or wall-clock time: clang records the source path into the emitted
/// object (on ELF, into the object bytes themselves), so a pid/nanos/counter
/// in the `.ll` name made two identical compiles produce different objects on
/// Linux (#7131). The `.ll` is content-addressed instead; uniqueness of
/// concurrent same-content writes is handled by an atomic rename.
///
/// Which names actually reach the object, measured rather than assumed
/// (aarch64 Debian clang 19.1.7, ELF, no `-g`) — so nobody has to re-derive
/// this when reviewing a temp-path change:
///
/// | name                            | recorded in the `.o`?          |
/// |---------------------------------|--------------------------------|
/// | `.ll` **basename**              | YES — `STT_FILE` in `.symtab`  |
/// | `.ll` **directory**, process CWD| no (needs DWARF, i.e. `-g`)    |
/// | `-o` output path                | no                             |
/// | `ld -r` input / output paths    | no (`compile_units_to_object`) |
///
/// That is the whole reason only the `.ll` basename had to change: the counter
/// may stay in every *output* name, where it costs nothing and still closes
/// #509. And because the *directory* is recorded nowhere, #7144 could put every
/// compile's `.ll` in a directory of its own and delete it again.
///
/// **`PERRY_DEBUG_SYMBOLS` is not an exception**, contrary to what the comment
/// here used to say. `-g` was assumed to pull the `.ll`'s absolute path plus
/// `DW_AT_comp_dir` into DWARF, which would have made the file part of the
/// shipped object and forced it to persist at a stable path. Measured on a real
/// Perry module (Apple clang 21, `-target x86_64-unknown-linux-gnu` and
/// `aarch64-unknown-linux-gnu`): the `-g` object is **byte-identical** to the
/// one without it and carries **no `.debug_*` sections at all**. Perry's codegen
/// emits no `DICompileUnit`/`DIFile`/`!dbg` metadata, and `clang -g` on a `.ll`
/// lowers debug info that is in the IR rather than synthesising a compile unit
/// for the input file. So `-g` records nothing about where the `.ll` lived, and
/// the temp-file lifetime does not depend on it — see
/// `debug_symbols_do_not_change_what_the_object_records`. (Not measured on
/// COFF/Windows.)
static TEMP_NONCE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// The environment inputs that decide what happens to the temp files.
///
/// Read once, in `compile_ll_to_object`, and threaded down rather than probed
/// where they are used: the lifecycle is then testable without a test mutating
/// process-wide environment underneath every other test in the binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TempFilePolicy {
    /// `PERRY_LLVM_KEEP_IR` — retain every intermediate and write the compile
    /// plan alongside it. The only input that changes the files' lifetime.
    keep: bool,
    /// `PERRY_DEBUG_SYMBOLS` — clang gets `-g`. Carried here only so the flag
    /// is a parameter rather than an env probe buried in plan construction; it
    /// deliberately does **not** affect cleanup (see `TEMP_NONCE_COUNTER`).
    debug_symbols: bool,
}

impl TempFilePolicy {
    fn from_env() -> Self {
        Self {
            keep: env::var_os("PERRY_LLVM_KEEP_IR").is_some(),
            debug_symbols: env::var_os("PERRY_DEBUG_SYMBOLS").is_some(),
        }
    }
}

/// FNV-1a 64-bit over `ll_text`. Stable across platforms and rustc versions
/// (unlike `DefaultHasher`), used only to content-address temp IR filenames so
/// clang embeds a deterministic source path (#7131). Collision risk is
/// acceptable for `/tmp` scratch files of compiler IR.
fn ll_content_hash(ll_text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in ll_text.as_bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// Content-addressed `.ll` path + per-process-unique `.o` path under `tmp_dir`.
///
/// The two names are asymmetric **on purpose**, and each half has already been
/// got wrong once:
///
/// * The `.ll` basename is a function of the IR bytes ALONE. clang records a
///   translation unit's source basename into the object, so anything else in
///   this name (pid, clock) lands in the shipped bytes — that was #7131.
/// * The `.o` basename must be unique per *process as well as* per call. The
///   output path is not recorded anywhere, so uniquifiers are free here, and
///   they are mandatory: `compile_ll_to_object` deletes the object once it has
///   read it, so two concurrent `perry` processes compiling identical IR that
///   agree on the name will delete it out from under each other. The counter
///   alone does not achieve this — it is per-process state, and every process
///   starts it at 0, so two processes with the same IR both pick
///   `..._0.o`. Measured before this was fixed: **8 of 12** concurrent
///   same-source compiles failed with "Failed to read clang output … No such
///   file or directory". This is #509 again, one scope out.
///
/// Both names additionally sit inside a `scratch_dir` that belongs to this call
/// alone (#7144). The directory name is *not* content-addressed — it must not
/// be, or two callers would share it again and the unlink would be racing a
/// sibling, which is the corner #7135 painted itself into. And it does not have
/// to be: only the basename reaches the object.
///
/// `pid` and `counter` are parameters rather than read in here so the property
/// above is testable without spawning processes.
fn llvm_temp_paths_for(tmp_dir: &Path, ll_text: &str, pid: u32, counter: u64) -> LlvmTempPaths {
    let hash = ll_content_hash(ll_text);
    let ll_name = format!("perry_llvm_{hash:016x}.ll");
    // The `.o` keeps its uniquifiers even inside a private directory. They cost
    // nothing, and the day someone flattens the layout again the object must
    // not silently go back to colliding across processes (#7140).
    let obj_name = format!("perry_llvm_{hash:016x}_{pid:x}_{counter:x}.o");
    let scratch = tmp_dir.join(format!("perry_llvm_scratch_{pid:x}_{counter:x}"));
    LlvmTempPaths {
        ll_path: scratch.join(&ll_name),
        obj_path: scratch.join(&obj_name),
        scratch_dir: scratch,
    }
}

/// Temp paths for one `compile_ll_to_object` call.
#[derive(Debug, Clone)]
struct LlvmTempPaths {
    /// Directory owned exclusively by this call, removed once the object bytes
    /// have been read.
    scratch_dir: PathBuf,
    ll_path: PathBuf,
    obj_path: PathBuf,
}

/// `llvm_temp_paths_for` with this process's pid and the next counter value.
/// Returns the paths plus `(pid, counter)` — the last two also name the
/// atomic-write staging file.
fn llvm_temp_paths(tmp_dir: &Path, ll_text: &str) -> (LlvmTempPaths, u32, u64) {
    let pid = std::process::id();
    let counter = TEMP_NONCE_COUNTER.fetch_add(1, Ordering::Relaxed);
    (
        llvm_temp_paths_for(tmp_dir, ll_text, pid, counter),
        pid,
        counter,
    )
}

/// Staging name for the atomic `.ll` write. Must be unique per process for the
/// same reason the `.o` is: two processes holding identical IR reach this with
/// the same content hash and the same counter value.
fn ll_staging_path(ll_path: &Path, pid: u32, counter: u64) -> PathBuf {
    ll_path.with_extension(format!("ll.tmp.{pid:x}.{counter:x}"))
}

/// Write `ll_text` to a content-addressed path. Concurrent workers with the
/// same IR may race; we write via a unique `.tmp` then `rename` into place so
/// readers never see a partial file. A lost race (dest already exists) is fine
/// — the winner already wrote the same content.
fn write_ll_atomically(ll_path: &Path, ll_text: &str, pid: u32, counter: u64) -> Result<()> {
    // Fast path: already present (common under parallel multi-module compile
    // when two units share nothing but we re-hit the same hash only on true
    // content match — overwrite is still safe because the content is identical).
    if ll_path.is_file() {
        // Refresh contents in case a stale hash collision left wrong bytes
        // (vanishingly rare). Same-content rewrite is a no-op for readers that
        // already hold a descriptor open.
        if let Ok(existing) = fs::read(ll_path) {
            if existing == ll_text.as_bytes() {
                return Ok(());
            }
        }
    }
    let tmp = ll_staging_path(ll_path, pid, counter);
    {
        let mut f = fs::File::create(&tmp)
            .with_context(|| format!("Failed to create temp .ll file at {}", tmp.display()))?;
        f.write_all(ll_text.as_bytes())?;
    }
    match fs::rename(&tmp, ll_path) {
        Ok(()) => Ok(()),
        Err(_) => {
            // Another worker won the rename, or dest exists. Prefer dest if it
            // already has the right bytes; otherwise fall back to a direct write.
            let _ = fs::remove_file(&tmp);
            if let Ok(existing) = fs::read(ll_path) {
                if existing == ll_text.as_bytes() {
                    return Ok(());
                }
            }
            fs::write(ll_path, ll_text.as_bytes())
                .with_context(|| format!("Failed to write temp .ll file at {}", ll_path.display()))
        }
    }
}

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

/// Resolve the CPU-tuning clang flag for one `.ll` → `.o` compile (#6125).
///
/// `requested` is the value of `PERRY_TARGET_CPU` — set directly, or promoted
/// by the compile driver from `--march` / perry.toml `[build] march` /
/// `[build] native_tuning`:
///
///   - `None` (unset) — historical default: a host build (no explicit target
///     triple) tunes to the build machine via `-march=native`/`-mcpu=native`;
///     an explicit-triple build gets no tuning flag (the target's portable
///     baseline).
///   - `"native"` — force build-machine tuning.
///   - `"generic"` / `"off"` / `"none"` / `"0"` / `"false"` — no tuning flag:
///     the target architecture's portable baseline, even for host builds.
///   - anything else — an explicit LLVM CPU name (`x86-64-v2`, `x86-64-v3`,
///     `znver2`, `apple-m1`, …) the emitted code must not exceed.
///
/// The flag spelling follows the EFFECTIVE target, not the host: x86 targets
/// take `-march=<cpu>`, everything else (aarch64/arm, riscv) `-mcpu=<cpu>`.
/// This knob exists because binaries built on one machine and run on another
/// (the `perry publish` hub, shared CI caches) must not bake the build box's
/// full instruction set — e.g. AVX-512 — into the shipped objects, which
/// SIGILLs on any CPU missing those extensions.
fn cpu_tuning_arg_for(
    requested: Option<&str>,
    target_triple: Option<&str>,
    effective_target: &str,
) -> Option<String> {
    let arch_flag = |cpu: &str| {
        let is_x86 = effective_target.starts_with("x86_64")
            || effective_target.starts_with("i686")
            || effective_target.starts_with("i586");
        if is_x86 {
            format!("-march={cpu}")
        } else {
            format!("-mcpu={cpu}")
        }
    };
    match requested.map(str::trim).filter(|s| !s.is_empty()) {
        None => target_triple
            .is_none()
            .then(|| native_tuning_arg_for_host().to_string()),
        Some("generic") | Some("off") | Some("none") | Some("0") | Some("false") => None,
        Some(cpu) => Some(arch_flag(cpu)),
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
    debug_symbols: bool,
) -> ClangCompilePlan {
    let effective_target = target_triple
        .map(|s| s.to_string())
        .unwrap_or_else(crate::codegen::default_target_triple);
    let requested_cpu = std::env::var("PERRY_TARGET_CPU").ok();
    let native_tuning_arg =
        cpu_tuning_arg_for(requested_cpu.as_deref(), target_triple, &effective_target);
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
    // A parameter rather than an env probe so a test can pin what `-g` does
    // and does not reach — measured in #7144: on a Perry `.ll` it produces a
    // byte-identical object with no `.debug_*` sections, because Perry's
    // codegen emits no DI metadata for clang to lower. See `TEMP_NONCE_COUNTER`.
    if debug_symbols {
        clang_args.push("-g".to_string());
    }
    clang_args.push("-fno-math-errno".to_string());
    // Inline-hot-small (#6850 follow-up): raise LLVM's `-inlinehint-threshold`
    // so `inlinehint`-marked callees (Perry stamps that ONLY on small functions
    // with an in-loop call site — see codegen/function.rs) actually inline into
    // their hot loops. The default hint threshold (325) is below the ~800 cost
    // of a NaN-boxed bit-mixer kernel. This only lifts the ceiling for hinted
    // functions; every other function keeps the base -O3 threshold, so cold
    // code is untouched (the anti-bloat property). Only meaningful at -O3.
    if opt_flag == "-O3" && crate::codegen::helpers::inline_hot_small_enabled() {
        clang_args.push("-mllvm".to_string());
        clang_args.push(format!(
            "-inlinehint-threshold={}",
            crate::codegen::helpers::inline_hot_small_hint_threshold()
        ));
    }
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
    compile_ll_to_object_in(
        &env::temp_dir(),
        ll_text,
        target_triple,
        TempFilePolicy::from_env(),
    )
}

/// `compile_ll_to_object` with the temp root and the file-lifetime policy as
/// arguments instead of process state.
///
/// Both are parameters so the temp-file *lifecycle* — the subject of #7144 — is
/// testable: a test can hand this an empty directory of its own and assert what
/// is left in it, without racing every other test in the binary over `TMPDIR`
/// or `PERRY_LLVM_KEEP_IR`.
///
/// Cleanup policy, in one place:
///
/// * **success**: the object is read into memory, and the per-call directory
///   goes with it — a compile leaves nothing behind (#7144).
/// * **failure** (clang non-zero, or the object cannot be read): everything is
///   left on disk. The error message names the `.ll`, and a failed compile is
///   exactly when someone wants to look at the IR that produced it.
/// * **`PERRY_LLVM_KEEP_IR`**: everything is kept and its location printed,
///   plus the compile plan as JSON.
/// * **`PERRY_DEBUG_SYMBOLS`**: no effect on any of the above. It was believed
///   to put the `.ll`'s absolute path into DWARF; measured, it does not put
///   anything there at all. See `TEMP_NONCE_COUNTER`.
/// exp/llvm-inprocess Phase 2: plan argv for a natively-constructed module,
/// produced by the SAME decision code as the clang path so opt levels
/// (incl. the #4880 oversized fallback) and CPU tuning cannot drift. The
/// byte-size input is the render-free estimate (`estimated_ir_bytes`), since
/// a native module never renders; the paths in the argv are placeholders the
/// in-process interpreter skips.
#[cfg(feature = "llvm-inprocess")]
pub(crate) fn native_plan_args(
    target_triple: Option<&str>,
    est_ll_bytes: usize,
    ll_fn_count: usize,
) -> (String, Vec<String>) {
    let plan = build_clang_compile_plan(
        PathBuf::from("(in-process)"),
        PathBuf::from("(native-module)"),
        PathBuf::from("(native-object)"),
        target_triple,
        est_ll_bytes,
        ll_fn_count,
        env::var_os("PERRY_DEBUG_SYMBOLS").is_some(),
    );
    (plan.effective_target, plan.clang_args)
}

/// exp/llvm-inprocess: truthy `PERRY_LLVM_INPROCESS` routes `.ll -> .o`
/// through the LLVM C API inside this process (no clang subprocess, no `.ll`
/// on disk). The flag participates in both the build cache and the object
/// cache keys, so the two backends can never share a cached object.
fn inprocess_requested() -> bool {
    match env::var("PERRY_LLVM_INPROCESS").as_deref() {
        Ok("") | Ok("0") | Ok("off") | Ok("false") | Err(_) => false,
        Ok(_) => true,
    }
}

#[cfg(feature = "llvm-inprocess")]
fn compile_ll_inprocess_in(
    tmp_dir: &Path,
    ll_text: &str,
    target_triple: Option<&str>,
    policy: TempFilePolicy,
) -> Result<Vec<u8>> {
    let (paths, _pid, _nonce) = llvm_temp_paths(tmp_dir, ll_text);
    // Same decision inputs as the clang path — opt level (#4880 fallback
    // included), CPU tuning, inlinehint threshold — via the same plan
    // constructor, so the backends cannot drift on a decision independently.
    let plan = build_clang_compile_plan(
        PathBuf::from("(in-process)"),
        paths.ll_path.clone(),
        paths.obj_path.clone(),
        target_triple,
        ll_text.len(),
        count_ll_functions(ll_text),
        policy.debug_symbols,
    );
    // #7131 parity: the module identifier is the content-addressed basename,
    // the only name that can reach the object bytes.
    let module_name = paths
        .ll_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "perry_module.ll".to_string());
    if policy.keep {
        fs::create_dir_all(&paths.scratch_dir)
            .with_context(|| format!("Failed to create {}", paths.scratch_dir.display()))?;
        fs::write(&paths.ll_path, ll_text)
            .with_context(|| format!("Failed to write {}", paths.ll_path.display()))?;
        let metadata_path = PathBuf::from(format!("{}.compile-plan.json", plan.obj_path.display()));
        write_compile_plan_metadata(&plan, &metadata_path)?;
        eprintln!("[perry-codegen] kept LLVM IR: {}", paths.ll_path.display());
        eprintln!(
            "[perry-codegen] kept compile metadata: {}",
            metadata_path.display()
        );
    }
    match crate::inprocess::compile_ll_to_object_inprocess(
        ll_text,
        &plan.effective_target,
        &plan.clang_args,
        &module_name,
    ) {
        Ok(bytes) => Ok(bytes),
        Err(e) => {
            // Same contract as a failed clang compile: the IR that produced
            // the failure is left on disk and named in the error.
            let _ = fs::create_dir_all(&paths.scratch_dir);
            let _ = fs::write(&paths.ll_path, ll_text);
            Err(anyhow!(
                "in-process LLVM compile failed (PERRY_LLVM_INPROCESS).\n\
                 requested -target: {}\n\
                 LLVM IR left at: {}\n\
                 \n\
                 {}",
                plan.effective_target,
                paths.ll_path.display(),
                e
            ))
        }
    }
}

#[cfg(not(feature = "llvm-inprocess"))]
fn compile_ll_inprocess_in(
    _tmp_dir: &Path,
    _ll_text: &str,
    _target_triple: Option<&str>,
    _policy: TempFilePolicy,
) -> Result<Vec<u8>> {
    // Fail loudly rather than silently falling back: an A/B arm that asked
    // for the in-process backend must never be served the text path.
    bail!(
        "PERRY_LLVM_INPROCESS is set, but this perry was built without the \
         `llvm-inprocess` cargo feature. Rebuild with \
         `cargo build -p perry --features llvm-inprocess` (needs LLVM 22: \
         `brew install llvm`, and LLVM_SYS_221_PREFIX if llvm-config is not \
         on PATH), or unset PERRY_LLVM_INPROCESS."
    )
}

fn compile_ll_to_object_in(
    tmp_dir: &Path,
    ll_text: &str,
    target_triple: Option<&str>,
    policy: TempFilePolicy,
) -> Result<Vec<u8>> {
    if inprocess_requested() {
        return compile_ll_inprocess_in(tmp_dir, ll_text, target_triple, policy);
    }
    // Validate the toolchain before creating the potentially large `.ll`
    // scratch file. Unsupported clang releases should fail without leaving
    // an artifact that was never passed to the compiler.
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
    ensure_supported_clang(&clang)?;

    // #7131: content-address the `.ll` basename (clang embeds it into the
    // object on ELF). #509: keep the `.o` unique via the per-call counter.
    // #7144: put both under a directory this call owns, so the `.ll` can be
    // deleted again without racing a sibling that hashed the same IR.
    let (paths, write_pid, write_nonce) = llvm_temp_paths(tmp_dir, ll_text);
    let LlvmTempPaths {
        scratch_dir,
        ll_path,
        obj_path,
    } = paths;
    fs::create_dir_all(&scratch_dir)
        .with_context(|| format!("Failed to create temp dir at {}", scratch_dir.display()))?;
    write_ll_atomically(&ll_path, ll_text, write_pid, write_nonce)?;

    let plan = build_clang_compile_plan(
        clang.clone(),
        ll_path.clone(),
        obj_path.clone(),
        target_triple,
        ll_text.len(),
        count_ll_functions(ll_text),
        policy.debug_symbols,
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
    // CPU tuning rides the same plan: by default only host builds receive
    // `-mcpu=native` / `-march=native`; `PERRY_TARGET_CPU` (from `--march` /
    // perry.toml `[build]`) overrides that with an explicit baseline or
    // disables tuning entirely. See `cpu_tuning_arg_for` (#6125).

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

    // Clean up on success.
    //
    // #7135 could not delete the `.ll`: it had just made the name a pure
    // function of the IR, so two workers holding identical IR *shared* that
    // path and either could unlink it in the window between the other computing
    // the path and clang opening it. The consequence (#7144) was that nothing
    // ever deleted them — one file per distinct IR ever compiled, measured at
    // 29 GB on a dev box.
    //
    // The fix is not a more careful delete, it is removing the sharing: the
    // `.ll` now sits in a directory that belongs to this call, so unlinking it
    // is unobservable to anyone else and there is no window to lose. The name
    // clang records — the basename — is untouched, so emission stays
    // deterministic (#7131).
    if policy.keep {
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
        // Ours alone: `remove_dir_all` rather than unlinking the two names we
        // know about, so anything clang chose to drop beside them goes too and
        // the directory cannot survive as an empty husk.
        //
        // Unconditional, including under `PERRY_DEBUG_SYMBOLS`: `-g` records
        // nothing about this path, measured — see `TEMP_NONCE_COUNTER`.
        let _ = fs::remove_dir_all(&scratch_dir);
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

/// Oldest clang release that accepts Perry's opaque-pointer LLVM IR without
/// an opt-in flag.
pub const MINIMUM_CLANG_MAJOR: u32 = 15;

/// Return the complete `clang --version` output, preferring stdout but
/// accepting wrappers that write their version banner to stderr.
pub fn clang_version_output(clang: &Path) -> Option<String> {
    let output = Command::new(clang).arg("--version").output().ok()?;
    select_clang_version_output(
        &String::from_utf8_lossy(&output.stdout),
        &String::from_utf8_lossy(&output.stderr),
    )
}

fn select_clang_version_output(stdout: &str, stderr: &str) -> Option<String> {
    let stdout = stdout.trim();
    let stderr = stderr.trim();
    if parse_clang_major_version(stdout).is_some() {
        return Some(stdout.to_string());
    }
    if parse_clang_major_version(stderr).is_some() {
        return Some(stderr.to_string());
    }
    if !stdout.is_empty() {
        return Some(stdout.to_string());
    }
    (!stderr.is_empty()).then(|| stderr.to_string())
}

/// Parse the major release from standard clang version banners, including
/// distro-prefixed and Apple clang variants.
pub fn parse_clang_major_version(version_output: &str) -> Option<u32> {
    version_output.lines().find_map(|line| {
        let marker = "clang version ";
        let start = line.find(marker)? + marker.len();
        let digits: String = line[start..]
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect();
        if digits.is_empty() {
            None
        } else {
            digits.parse().ok()
        }
    })
}

fn clang_major_version(clang: &Path) -> Option<u32> {
    clang_version_output(clang).and_then(|output| parse_clang_major_version(&output))
}

fn ensure_supported_clang(clang: &Path) -> Result<()> {
    ensure_supported_clang_major(clang, clang_major_version(clang))
}

fn ensure_supported_clang_major(clang: &Path, major: Option<u32>) -> Result<()> {
    let Some(major) = major else {
        // Some toolchain wrappers do not expose a conventional version
        // banner. Let the real compilation attempt decide whether they work.
        return Ok(());
    };
    if major < MINIMUM_CLANG_MAJOR {
        bail!(
            "clang at `{}` is too old ({} < {}). Perry emits opaque-pointer LLVM IR, \
             which requires clang {} or newer. Install clang-{}+ and put it first on \
             PATH, or set PERRY_LLVM_CLANG to a supported clang binary.",
            clang.display(),
            major,
            MINIMUM_CLANG_MAJOR,
            MINIMUM_CLANG_MAJOR,
            MINIMUM_CLANG_MAJOR,
        );
    }
    Ok(())
}

fn select_clang_candidate_with<F>(
    candidates: impl IntoIterator<Item = PathBuf>,
    mut version_probe: F,
) -> Option<PathBuf>
where
    F: FnMut(&Path) -> Option<u32>,
{
    let mut fallback = None;
    for candidate in candidates {
        if fallback.is_none() {
            fallback = Some(candidate.clone());
        }
        if version_probe(&candidate).is_some_and(|major| major >= MINIMUM_CLANG_MAJOR) {
            return Some(candidate);
        }
    }
    fallback
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

    let mut candidates = Vec::new();
    // Keep the ordinary PATH spelling first when it is already supported.
    // If it points to clang 14 (Ubuntu 22.04), candidate selection continues
    // through versioned Debian/Ubuntu binaries before falling back to it.
    if let Some(candidate) = which_path("clang") {
        candidates.push(candidate);
    }
    for major in (MINIMUM_CLANG_MAJOR..=40).rev() {
        if let Some(candidate) = which_path(&format!("clang-{major}")) {
            if !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
    }
    // Keep old versioned-only installations discoverable so doctor/compiler
    // can report "too old" instead of the misleading "clang not found".
    for major in (3..MINIMUM_CLANG_MAJOR).rev() {
        if let Some(candidate) = which_path(&format!("clang-{major}")) {
            if !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
    }

    // Check well-known install locations.
    #[cfg(windows)]
    {
        // Standalone LLVM installer (llvm.org)
        let standalone = PathBuf::from(r"C:\Program Files\LLVM\bin\clang.exe");
        if standalone.exists() {
            candidates.push(standalone);
        }
        // MSVC Build Tools bundled clang (via "C++ Clang Compiler" component)
        if let Some(path) = find_msvc_bundled_clang() {
            candidates.push(path);
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
                candidates.push(candidate);
            }
        }
        for major in (MINIMUM_CLANG_MAJOR..=40).rev() {
            let candidate = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/clang"));
            if candidate.exists() && is_executable(&candidate) && !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
        for major in (3..MINIMUM_CLANG_MAJOR).rev() {
            let candidate = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/clang"));
            if candidate.exists() && is_executable(&candidate) && !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
    }

    select_clang_candidate_with(candidates, clang_major_version)
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

fn which_path(name: &str) -> Option<PathBuf> {
    let path_var = match env::var_os("PATH") {
        Some(p) => p,
        None => return None,
    };
    for dir in env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.exists() && is_executable(&candidate) {
            return Some(candidate);
        }
        // On Windows, executables have .exe extension
        #[cfg(windows)]
        {
            let with_exe = dir.join(format!("{}.exe", name));
            if with_exe.exists() && is_executable(&with_exe) {
                return Some(with_exe);
            }
        }
    }
    None
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
    if let Some(path) = which_path(tool) {
        return Some(path);
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
    fn parses_common_clang_version_banners() {
        assert_eq!(
            parse_clang_major_version("Ubuntu clang version 14.0.0-1ubuntu1.1"),
            Some(14)
        );
        assert_eq!(
            parse_clang_major_version("Apple clang version 17.0.0 (clang-1700.0.13.5)"),
            Some(17)
        );
        assert_eq!(
            parse_clang_major_version("Debian clang version 18.1.8\nTarget: x86_64-linux-gnu"),
            Some(18)
        );
        assert_eq!(parse_clang_major_version("not a clang banner"), None);
    }

    #[test]
    fn version_banner_on_stderr_wins_over_stdout_wrapper_noise() {
        let selected = select_clang_version_output(
            "wrapper: selecting system toolchain",
            "Ubuntu clang version 14.0.0-1ubuntu1.1",
        );
        assert_eq!(
            selected.as_deref(),
            Some("Ubuntu clang version 14.0.0-1ubuntu1.1")
        );
        assert_eq!(
            select_clang_version_output("clang version 18.1.8", "warning").as_deref(),
            Some("clang version 18.1.8")
        );
    }

    #[test]
    fn prefers_supported_versioned_clang_over_old_path_default() {
        let candidates = vec![
            PathBuf::from("/usr/bin/clang"),
            PathBuf::from("/usr/bin/clang-18"),
            PathBuf::from("/usr/bin/clang-15"),
        ];
        let selected =
            select_clang_candidate_with(candidates, |path| match path.file_name()?.to_str()? {
                "clang" => Some(14),
                "clang-18" => Some(18),
                "clang-15" => Some(15),
                _ => None,
            });
        assert_eq!(selected, Some(PathBuf::from("/usr/bin/clang-18")));
    }

    #[test]
    fn retains_first_candidate_to_report_an_old_only_install() {
        let candidates = vec![
            PathBuf::from("/usr/bin/clang"),
            PathBuf::from("/usr/bin/clang-14"),
        ];
        let selected = select_clang_candidate_with(candidates, |_| Some(14));
        assert_eq!(selected, Some(PathBuf::from("/usr/bin/clang")));
    }

    #[test]
    fn old_clang_preflight_explains_the_opaque_pointer_requirement() {
        let error = ensure_supported_clang_major(Path::new("/usr/bin/clang"), Some(14))
            .expect_err("clang 14 must be rejected");
        let message = error.to_string();
        assert!(message.contains("too old (14 < 15)"));
        assert!(message.contains("opaque-pointer LLVM IR"));
        assert!(message.contains("PERRY_LLVM_CLANG"));
        assert!(ensure_supported_clang_major(Path::new("/usr/bin/clang-15"), Some(15)).is_ok());
        assert!(ensure_supported_clang_major(Path::new("/toolchain-wrapper"), None).is_ok());
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
            false,
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
            false,
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
            false,
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
            false,
        );
        assert_eq!(plan.effective_target, "x86_64-unknown-linux-gnu");
        assert_eq!(plan.native_tuning_arg, None);
        assert!(!plan
            .clang_args
            .iter()
            .any(|arg| arg == "-march=native" || arg == "-mcpu=native"));
    }

    #[test]
    fn cpu_tuning_unset_keeps_historical_defaults() {
        // Host build (no triple) → native tuning; explicit triple → none.
        assert_eq!(
            cpu_tuning_arg_for(None, None, "x86_64-apple-darwin").as_deref(),
            Some(native_tuning_arg_for_host())
        );
        assert_eq!(
            cpu_tuning_arg_for(
                None,
                Some("x86_64-unknown-linux-gnu"),
                "x86_64-unknown-linux-gnu"
            ),
            None
        );
    }

    #[test]
    fn cpu_tuning_explicit_cpu_spells_march_or_mcpu_by_target_arch() {
        // #6125: an explicit baseline applies to host AND cross builds, and
        // the flag spelling follows the effective target's architecture.
        assert_eq!(
            cpu_tuning_arg_for(Some("x86-64-v2"), None, "x86_64-unknown-linux-gnu").as_deref(),
            Some("-march=x86-64-v2")
        );
        assert_eq!(
            cpu_tuning_arg_for(
                Some("x86-64-v3"),
                Some("x86_64-unknown-linux-musl"),
                "x86_64-unknown-linux-musl"
            )
            .as_deref(),
            Some("-march=x86-64-v3")
        );
        assert_eq!(
            cpu_tuning_arg_for(Some("apple-m1"), None, "arm64-apple-macosx15.0.0").as_deref(),
            Some("-mcpu=apple-m1")
        );
    }

    #[test]
    fn cpu_tuning_generic_disables_native_tuning_on_host_builds() {
        for off in ["generic", "off", "none", "0", "false"] {
            assert_eq!(
                cpu_tuning_arg_for(Some(off), None, "x86_64-apple-darwin"),
                None,
                "'{off}' should disable tuning"
            );
        }
        // Whitespace / empty values fall back to the default.
        assert_eq!(
            cpu_tuning_arg_for(Some("  "), None, "x86_64-apple-darwin").as_deref(),
            Some(native_tuning_arg_for_host())
        );
    }

    #[test]
    fn cpu_tuning_native_can_be_forced_for_explicit_triples() {
        assert_eq!(
            cpu_tuning_arg_for(
                Some("native"),
                Some("x86_64-unknown-linux-gnu"),
                "x86_64-unknown-linux-gnu"
            )
            .as_deref(),
            Some("-march=native")
        );
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
            false,
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
        // **output** temp-file path. The counter is mixed into the `.o`
        // basename (the `.ll` is content-addressed — see #7131).
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

    #[test]
    fn ll_temp_basename_is_content_addressed_not_clocked() {
        // #7131: two calls with identical IR must produce the same `.ll`
        // basename (so clang embeds a deterministic source path). The `.o`
        // basename still differs via the counter.
        let tmp = env::temp_dir();
        let ir = "define void @f() {\n  ret void\n}\n";
        let (a, _, _) = llvm_temp_paths(&tmp, ir);
        let (b, _, _) = llvm_temp_paths(&tmp, ir);
        let (ll_a, obj_a) = (&a.ll_path, &a.obj_path);
        let (ll_b, obj_b) = (&b.ll_path, &b.obj_path);
        assert_eq!(
            ll_a.file_name(),
            ll_b.file_name(),
            "same IR must share the content-addressed .ll basename"
        );
        assert_ne!(
            obj_a.file_name(),
            obj_b.file_name(),
            ".o basenames must stay unique across calls (#509)"
        );
        // Different IR → different .ll basename.
        let (c, _, _) = llvm_temp_paths(&tmp, "define void @g() {\n  ret void\n}\n");
        assert_ne!(ll_a.file_name(), c.ll_path.file_name());
        // No pid / wall-clock digits of variable width — only hex hash.
        let name = ll_a.file_name().unwrap().to_string_lossy();
        assert!(
            name.starts_with("perry_llvm_") && name.ends_with(".ll"),
            "unexpected .ll name: {name}"
        );
        let hex = name
            .trim_start_matches("perry_llvm_")
            .trim_end_matches(".ll");
        assert_eq!(hex.len(), 16, "hash must be 16 lowercase hex digits: {hex}");
        assert!(
            hex.chars().all(|c| c.is_ascii_hexdigit()),
            "hash must be hex: {hex}"
        );
    }

    #[test]
    fn object_temp_name_is_unique_across_processes_but_ll_is_not() {
        // The regression this test exists for: #7135 content-addressed BOTH
        // temp names, so the `.o` lost the pid it used to carry. Two `perry`
        // processes compiling identical IR then agreed on the object path —
        // and `compile_ll_to_object` deletes the object after reading it, so
        // they deleted each other's. Measured on macOS before the fix: 8 of 12
        // concurrent same-source compiles failed with
        //   Failed to read clang output at …/perry_llvm_<hash>_0.o
        // Both processes start TEMP_NONCE_COUNTER at 0, so the counter cannot
        // separate them; only the pid can.
        let tmp = env::temp_dir();
        let ir = "define void @f() {\n  ret void\n}\n";

        // Same IR, same counter, DIFFERENT process.
        let p1 = llvm_temp_paths_for(&tmp, ir, 1111, 0);
        let p2 = llvm_temp_paths_for(&tmp, ir, 2222, 0);
        let (ll_p1, obj_p1) = (&p1.ll_path, &p1.obj_path);
        let (ll_p2, obj_p2) = (&p2.ll_path, &p2.obj_path);
        assert_eq!(
            ll_p1.file_name(),
            ll_p2.file_name(),
            "the .ll is what clang records into the object; it must stay a pure \
             function of the IR across processes (#7131)"
        );
        assert_ne!(
            obj_p1.file_name(),
            obj_p2.file_name(),
            "two processes with identical IR must NOT share an object path — \
             they delete it out from under each other (#509 across processes)"
        );

        // Same process, different call: the counter still has to separate
        // in-process rayon workers.
        let c0 = llvm_temp_paths_for(&tmp, ir, 1111, 0);
        let c1 = llvm_temp_paths_for(&tmp, ir, 1111, 1);
        assert_ne!(c0.obj_path.file_name(), c1.obj_path.file_name());

        // The atomic-write staging name needs the same separation: both
        // processes reach it with the same hash and the same counter, and
        // `File::create` truncates.
        assert_ne!(
            ll_staging_path(ll_p1, 1111, 0).file_name(),
            ll_staging_path(ll_p1, 2222, 0).file_name(),
            "staging .tmp name must be per-process"
        );
        assert_ne!(
            ll_staging_path(ll_p1, 1111, 0).file_name(),
            ll_staging_path(ll_p1, 1111, 1).file_name(),
            "staging .tmp name must be per-call"
        );

        // …and the staging file must never be mistaken for the real `.ll`.
        assert_ne!(
            ll_staging_path(ll_p1, 1111, 0).file_name(),
            ll_p1.file_name()
        );
    }

    #[test]
    fn ll_content_hash_is_stable_for_fixed_input() {
        // Pin the FNV-1a value so a future hash swap is intentional.
        assert_eq!(ll_content_hash(""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(ll_content_hash("a"), 0xaf63_dc4c_8601_ec8c);
    }
}

/// Temp-file *lifecycle* — who owns the `.ll`, and when it is removed (#7144).
/// A sibling file only because of the 2,000-line cap; `use super::*` gives it
/// the same view of this module as the block above.
#[cfg(test)]
#[path = "linker_temp_lifecycle_tests.rs"]
mod linker_temp_lifecycle_tests;
