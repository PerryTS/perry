//! In-process `.ll -> .o` compilation through the LLVM C API (exp/llvm-inprocess).
//!
//! Feature-gated (`llvm-inprocess`) and flag-gated (`PERRY_LLVM_INPROCESS=1`):
//! the default build does not link LLVM, and a build that has the feature
//! still uses the `clang -c` subprocess unless the flag is set. Selection and
//! the flag's cache-key participation live in `linker.rs` /
//! `perry/src/commands/compile/{build_cache,object_cache}.rs`.
//!
//! Decision parity by construction: this module does not re-derive optimization
//! or CPU tuning. It interprets the *same* argv `build_clang_compile_plan`
//! produces for clang (`-O3`/explicit `-Os`, `-mcpu=native`, `-mllvm
//! -inlinehint-threshold=N`, `-target <triple>`), so the two backends cannot
//! drift on a decision without drifting on the plan — which the plan's own
//! tests pin.
//!
//! Measured in Phase 0 (see `docs/llvm-inprocess-experiment.md`): on the same
//! IR and flags this pipeline produces objects byte-identical to Homebrew
//! clang 22's `clang -c`.

mod optimize_emit;
use optimize_emit::optimize_and_emit;

use std::ffi::CString;
use std::sync::Once;

use anyhow::{anyhow, Result};
use inkwell::context::Context;
use inkwell::memory_buffer::MemoryBuffer;
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine, TargetTriple,
};
use inkwell::values::AsValueRef;
use inkwell::OptimizationLevel;

use crate::linker::STATEPOINT_REWRITE_PASSES;

/// Test seam (#7502): parse `ll_text`, run [`STATEPOINT_REWRITE_PASSES`] for
/// `effective_target`, and return the rewritten IR.
///
/// Everything about the target machine — triple, CPU, data layout — comes from
/// the same helpers `optimize_and_emit` uses, so an assertion here is about the
/// lowering that ships rather than about a pipeline assembled for the test.
/// Both verifies are load-bearing: the first rejects IR codegen should never
/// have emitted, the second rejects a statepoint form LLVM would refuse to
/// codegen (that is how the Itanium landing-pad shape was found).
#[cfg(test)]
pub(crate) fn statepoint_rewritten_ir(
    ll_text: &str,
    effective_target: &str,
    module_name: &str,
) -> Result<String> {
    statepoint_rewritten_ir_with_passes(
        ll_text,
        effective_target,
        module_name,
        STATEPOINT_REWRITE_PASSES,
    )
}

#[cfg(test)]
pub(crate) fn statepoint_rewritten_ir_with_passes(
    ll_text: &str,
    effective_target: &str,
    module_name: &str,
    passes: &str,
) -> Result<String> {
    global_init(&[]);
    let context = Context::create();
    let module = parse_ir_text(&context, ll_text, module_name)?;
    let triple = TargetTriple::create(effective_target);
    let target = Target::from_triple(&triple)
        .map_err(|e| anyhow!("no LLVM target for `{effective_target}`: {e}"))?;
    let tm = target
        .create_target_machine(
            &triple,
            default_cpu_for_triple(effective_target),
            "",
            OptimizationLevel::None,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or_else(|| anyhow!("failed to create TargetMachine for `{effective_target}`"))?;
    module.set_triple(&triple);
    module.set_data_layout(&tm.get_target_data().get_data_layout());
    module
        .verify()
        .map_err(|e| anyhow!("LLVM verifier rejected pre-statepoint module:\n{}", e))?;
    module
        .run_passes(passes, &tm, PassBuilderOptions::create())
        .map_err(|e| anyhow!("`{passes}` failed:\n{}", e))?;
    module
        .verify()
        .map_err(|e| anyhow!("LLVM verifier rejected the statepoint module:\n{}", e))?;
    Ok(module.print_to_string().to_string())
}

/// One-time process-global LLVM setup: target registration and `-mllvm`
/// pass-through flags. Both are process-global in LLVM itself, which is why
/// they are applied under a `Once` and not per compile. The `-mllvm` value is
/// captured from the first compile that carries one; Perry only ever passes a
/// single, env-derived `-inlinehint-threshold` value per process, so
/// first-wins is not a narrowing. (A future per-function-opt backend must
/// replace the cl::opt mechanism entirely — noted in the experiment doc.)
static LLVM_GLOBAL_INIT: Once = Once::new();
static ANNOUNCE: Once = Once::new();

fn global_init(mllvm: &[String]) {
    LLVM_GLOBAL_INIT.call_once(|| {
        // Only the backends Perry can actually emit for. `initialize_all()`
        // references every LLVM target's init symbol, which makes the static
        // link pull in all ~20 backends — measured at **+86.9 MB** on the
        // `perry` binary (185.9 MB -> 98.9 MB), 47% of the whole feature
        // build, for backends nothing can reach. It was inkwell's convenient
        // default in #7301, not a considered choice; the feature was opt-in so
        // nobody paid for it.
        //
        // Perry's LLVM target surface is exactly two architectures. Every
        // triple the compile driver can produce is aarch64 (Apple platforms,
        // Android, Linux gnu/musl/ohos, and watchOS's ILP32 `arm64_32`, which
        // is still the AArch64 backend) or x86 (`x86_64`, `x86_64h`, `i686`).
        // The lone `riscv64gc` string in the tree is a unit-test assertion in
        // `gc_map.rs`, not an emission target, and wasm has its own crate
        // (`perry-codegen-wasm`) that never reaches this backend.
        //
        // A triple outside this set fails loudly at `Target::from_triple`
        // ("no LLVM target for ..."), so adding an architecture without
        // initializing its backend is a hard error, never a silent fallback.
        let cfg = InitializationConfig::default();
        Target::initialize_aarch64(&cfg);
        Target::initialize_x86(&cfg);
        if !mllvm.is_empty() {
            let mut argv: Vec<CString> = vec![CString::new("perry-llvm-inprocess").unwrap()];
            for flag in mllvm {
                if let Ok(c) = CString::new(flag.as_str()) {
                    argv.push(c);
                }
            }
            let ptrs: Vec<*const std::os::raw::c_char> = argv.iter().map(|c| c.as_ptr()).collect();
            unsafe {
                llvm_sys::support::LLVMParseCommandLineOptions(
                    ptrs.len() as i32,
                    ptrs.as_ptr(),
                    std::ptr::null(),
                );
            }
        }
    });
}

/// The liveness witness ("never trust a green that cannot fail"): an A/B arm
/// claiming to be in-process must show this line on stderr.
fn announce() {
    ANNOUNCE.call_once(|| {
        let (mut major, mut minor, mut patch) = (0u32, 0u32, 0u32);
        unsafe { llvm_sys::core::LLVMGetVersion(&mut major, &mut minor, &mut patch) };
        eprintln!("perry: in-process LLVM backend active (LLVM {major}.{minor}.{patch})");
    });
}

/// Compile IR text to object bytes in-process, honoring the clang-style argv
/// from `build_clang_compile_plan`. `module_name` becomes the module
/// identifier (the deterministic content-addressed basename, mirroring #7131's
/// contract that only the IR bytes decide what lands in the object).
pub fn compile_ll_to_object_inprocess(
    ll_text: &str,
    effective_target: &str,
    clang_style_args: &[String],
    module_name: &str,
    native_roots: bool,
) -> Result<Vec<u8>> {
    let (opt, mcpu_native, explicit_cpu, mllvm, emit_asm) = interpret_plan_args(clang_style_args)?;
    // Same guard as the external `opt` path (`linker::rs4gc_funclet_refusal`):
    // rewrite-statepoints-for-gc crashes on WinEH funclet pads, and here the
    // pass runs inside THIS process — the crash would take the compiler down
    // with it, not just a child.
    if native_roots {
        if let Some(refusal) = crate::linker::rs4gc_funclet_refusal(ll_text) {
            return Err(anyhow!(refusal));
        }
    }
    let context = Context::create();
    let module = parse_ir_text(&context, ll_text, module_name)?;
    optimize_and_emit(
        &module,
        effective_target,
        opt,
        mcpu_native,
        explicit_cpu.as_deref(),
        &mllvm,
        emit_asm,
        native_roots,
        None,
    )
}

/// The CPU an empty `-mcpu` means for this triple.
///
/// LLVM's `create_target_machine` with an empty CPU selects `generic`, which on
/// aarch64 is **ARMv8.0**. Clang does not do that: for an Apple arm64 triple it
/// defaults to `apple-m1` (ARMv8.5). The gap is not academic — codegen decides
/// whether to emit `llvm.aarch64.fjcvtzs` (FEAT_JSCVT, ARMv8.3+, the
/// single-instruction ECMAScript `ToInt32`) from the TRIPLE ALONE, in
/// `codegen::helpers::set_jscvt_for_target`, precisely because clang's default
/// for that triple has the feature. Handing the same IR to a `generic`
/// TargetMachine gives `LLVM ERROR: Cannot select: intrinsic
/// %llvm.aarch64.fjcvtzs` and aborts the compile.
///
/// So this is the second half of a pair: `set_jscvt_for_target` decides what to
/// EMIT from the triple, and this decides what the target can EXECUTE from the
/// same triple. They must agree. If a triple is added to one, add it to the
/// other — a mismatch is a hard abort at `-O`-time, not a silent miscompile,
/// which is the one mercy here.
fn default_cpu_for_triple(triple: &str) -> &'static str {
    let is_aarch64 = triple.starts_with("arm64") || triple.starts_with("aarch64");
    let is_apple = triple.contains("apple");
    if is_aarch64 && is_apple {
        // Matches clang's default for arm64-apple-*, and is the assumption
        // `set_jscvt_for_target` already bakes in for macOS/darwin.
        "apple-m1"
    } else {
        // Every other triple keeps LLVM's portable baseline, which is what the
        // clang path gets too when no tuning flag is passed.
        ""
    }
}

/// Interpret the plan argv. Unknown dash-flags are an error on purpose:
/// silently ignoring a flag clang would have honored is how the two
/// backends drift apart without anyone noticing.
#[allow(clippy::type_complexity)]
fn interpret_plan_args(
    clang_style_args: &[String],
) -> Result<(char, bool, Option<String>, Vec<String>, bool)> {
    let mut opt = '0';
    // `-S` asks for assembly rather than an object. The statepoint backends
    // need it: #7314's compact-map rewriter rewrites `.llvm_stackmaps` at
    // ASSEMBLY time, where LLVM prints function addresses as symbol names, so
    // one text parser replaces Mach-O and ELF relocation parsing plus a second
    // link pass. Emitting an object here would skip that rewrite entirely.
    let mut emit_asm = false;
    let mut mcpu_native = false;
    let mut explicit_cpu: Option<String> = None;
    let mut mllvm: Vec<String> = Vec::new();
    let mut it = clang_style_args.iter().peekable();
    while let Some(a) = it.next() {
        match a.as_str() {
            // `-g` is a measured no-op on Perry IR (no DI metadata; see the
            // TEMP_NONCE_COUNTER doc block in linker.rs), matching clang.
            "-c" | "-fno-math-errno" | "-g" => {}
            "-S" => emit_asm = true,
            "-o" | "-target" => {
                it.next();
            }
            "-mllvm" => {
                if let Some(f) = it.next() {
                    mllvm.push(f.clone());
                }
            }
            "-mcpu=native" | "-march=native" => mcpu_native = true,
            s if s.starts_with("-mcpu=") => explicit_cpu = Some(s["-mcpu=".len()..].to_string()),
            s if s.starts_with("-march=") => explicit_cpu = Some(s["-march=".len()..].to_string()),
            s if s.starts_with("-O") => opt = s.chars().nth(2).unwrap_or('0'),
            s if !s.starts_with('-') => {} // input/output paths from the plan
            other => {
                return Err(anyhow!(
                    "in-process backend does not understand clang arg `{other}`; \
                     refusing to silently drop it"
                ))
            }
        }
    }
    Ok((opt, mcpu_native, explicit_cpu, mllvm, emit_asm))
}

/// Parse IR text into a module in `context`. Shared by the transport path
/// (whole-module text) and the native-construction path (the few-KB module
/// skeleton from `LlModule::skeleton_ir`).
pub(crate) fn parse_ir_text<'ctx>(
    context: &'ctx Context,
    ll_text: &str,
    module_name: &str,
) -> Result<inkwell::module::Module<'ctx>> {
    // inkwell 0.9's copy constructor requires (and then strips) a trailing
    // NUL. One extra copy of the IR text; fine for the transport phase.
    let mut ir = Vec::with_capacity(ll_text.len() + 1);
    ir.extend_from_slice(ll_text.as_bytes());
    ir.push(0);
    let buf = MemoryBuffer::create_from_memory_range_copy(&ir, module_name);
    context
        .create_module_from_ir(buf)
        .map_err(|e| anyhow!("LLVM IR parse error:\n{}", e.to_string()))
}

/// Interpret plan argv (same grammar as `compile_ll_to_object_inprocess`) and
/// run verify -> pass pipeline -> object emission on an already-built module.
/// The native construction path calls this directly.
pub(crate) fn optimize_and_emit_module(
    module: &inkwell::module::Module<'_>,
    effective_target: &str,
    clang_style_args: &[String],
    native_roots: bool,
) -> Result<Vec<u8>> {
    optimize_and_emit_module_with_stats(
        module,
        effective_target,
        clang_style_args,
        native_roots,
        None,
    )
}

/// [`optimize_and_emit_module`] that also fills `stats` (sizes before and
/// after RS4GC, widest functions, phase times, and any bounded-emission
/// fallback) for the per-unit report.
pub(crate) fn optimize_and_emit_module_with_stats(
    module: &inkwell::module::Module<'_>,
    effective_target: &str,
    clang_style_args: &[String],
    native_roots: bool,
    stats: Option<&mut UnitCodegenStats>,
) -> Result<Vec<u8>> {
    let (opt, mcpu_native, explicit_cpu, mllvm, emit_asm) = interpret_plan_args(clang_style_args)?;
    optimize_and_emit(
        module,
        effective_target,
        opt,
        mcpu_native,
        explicit_cpu.as_deref(),
        &mllvm,
        emit_asm,
        native_roots,
        stats,
    )
}

/// Per-unit facts the backend learns while it works: instruction totals and
/// the widest function before and after `rewrite-statepoints-for-gc`, and the
/// time each phase took. `native_emit` prints one line per unit from these
/// under `PERRY_CODEGEN_UNIT_TIMINGS`, so a build that is stuck in LLVM names
/// the function it is stuck on instead of a unit number (#8583).
#[derive(Debug, Default, Clone)]
pub struct UnitCodegenStats {
    pub functions: usize,
    pub pre_rewrite_instructions: usize,
    pub pre_rewrite_widest: Option<(String, usize)>,
    pub post_rewrite_instructions: usize,
    pub post_rewrite_widest: Option<(String, usize)>,
    pub rewrite_secs: f64,
    pub optimize_secs: f64,
    pub emit_secs: f64,
    /// Functions stamped `"disable-tail-calls"` because their alloca-walk
    /// estimate exceeded [`DEFAULT_TRE_MAX_ALLOCA_WALK`] (#8883).
    pub tail_call_elim_skipped: Vec<TreWalkOverBudget>,
    /// The widest function which made this unit use LLVM's bounded O0 machine
    /// pipeline after completing the requested IR optimization pipeline.
    pub fast_emit_fallback: Option<FastEmitFallback>,
}

fn function_instruction_count(function: inkwell::values::FunctionValue<'_>) -> usize {
    let mut instrs = 0usize;
    for bb in function.get_basic_blocks() {
        let mut inst = bb.get_first_instruction();
        while let Some(i) = inst {
            instrs += 1;
            inst = i.get_next_instruction();
        }
    }
    instrs
}

/// (defined functions, total instructions, widest function) for a module.
/// One linear walk through the C API; a few milliseconds per ordinary unit.
fn module_instruction_census(
    module: &inkwell::module::Module<'_>,
) -> (usize, usize, Option<(String, usize)>) {
    let mut functions = 0usize;
    let mut total = 0usize;
    let mut widest: Option<(String, usize)> = None;
    let mut function = module.get_first_function();
    while let Some(f) = function {
        if f.count_basic_blocks() > 0 {
            functions += 1;
            let n = function_instruction_count(f);
            total += n;
            if widest.as_ref().is_none_or(|(_, w)| n > *w) {
                widest = Some((f.get_name().to_string_lossy().into_owned(), n));
            }
        }
        function = f.get_next_function();
    }
    (functions, total, widest)
}

/// Per-function instruction ceiling for LLVM's optimized machine pipeline.
///
/// This budget is checked *after* the requested `default<O*>` IR pipeline has
/// completed. It changes neither JS lowering nor middle-end optimization; it
/// only asks the target machine to use its O0 instruction-selection,
/// live-interval and register-allocation pipeline for a unit containing an
/// extreme generated function.
///
/// The threshold is bracketed by real arm64/LLVM 22 measurements. Machine-IR
/// expansion depends on CFG shape, so raw IR size is deliberately only a
/// conservative guard: one 161k-instruction function emitted normally in
/// ~19s, while a different 100,152-instruction Claude Code 2.1.259 function
/// grew past ~10 GiB RSS in the optimized machine pipeline. The same function
/// emitted through an O0 target machine in 6s. Another 277k-instruction async
/// state-machine function remained in LiveIntervals / register allocation for
/// more than 16 minutes at ~10 GiB RSS; its already-Os-optimized IR emitted
/// through an O0 target machine in 3.5s at ~550 MiB RSS. 100k is immediately
/// below the smallest observed pathological case.
///
/// `PERRY_LL_FAST_EMIT_MAX_INSTRS=<n>` raises or lowers the ceiling; `0` /
/// `off` disables the fallback.
const DEFAULT_FAST_EMIT_MAX_INSTRS: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FastEmitBudget {
    Off,
    Cap(usize),
}

fn parse_fast_emit_budget(value: Option<&str>) -> FastEmitBudget {
    match value.map(str::trim) {
        None | Some("") => FastEmitBudget::Cap(DEFAULT_FAST_EMIT_MAX_INSTRS),
        Some("0") | Some("off") | Some("false") => FastEmitBudget::Off,
        Some(v) => match v.parse::<usize>() {
            Ok(0) => FastEmitBudget::Off,
            Ok(n) => FastEmitBudget::Cap(n),
            Err(_) => FastEmitBudget::Cap(DEFAULT_FAST_EMIT_MAX_INSTRS),
        },
    }
}

fn fast_emit_budget() -> FastEmitBudget {
    #[cfg(test)]
    if let Some(budget) = TEST_FAST_EMIT_BUDGET.with(std::cell::Cell::get) {
        return budget;
    }
    parse_fast_emit_budget(
        std::env::var("PERRY_LL_FAST_EMIT_MAX_INSTRS")
            .ok()
            .as_deref(),
    )
}

#[cfg(test)]
thread_local! {
    static TEST_FAST_EMIT_BUDGET: std::cell::Cell<Option<FastEmitBudget>> = const {
        std::cell::Cell::new(None)
    };
}

/// Thread-local budget seam; mutating the process environment would race the
/// other LLVM tests in this binary.
#[cfg(test)]
fn with_test_fast_emit_budget<T>(cap: usize, run: impl FnOnce() -> T) -> T {
    struct Restore(Option<FastEmitBudget>);
    impl Drop for Restore {
        fn drop(&mut self) {
            TEST_FAST_EMIT_BUDGET.with(|budget| budget.set(self.0));
        }
    }
    let old = TEST_FAST_EMIT_BUDGET.replace(Some(FastEmitBudget::Cap(cap)));
    let _restore = Restore(old);
    run()
}

/// The extreme function which selected bounded machine-code emission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FastEmitFallback {
    pub name: String,
    pub instructions: usize,
    pub cap: usize,
}

impl std::fmt::Display for FastEmitFallback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "`{}` has {} instructions after IR optimization, above the optimized machine-pipeline \
             budget {}; keeping the requested IR optimization, then emitting this unit through \
             LLVM's O0 machine pipeline to bound instruction selection, live intervals and \
             register allocation. Override with PERRY_LL_FAST_EMIT_MAX_INSTRS=<n> (raise) or \
             =0 (disable).",
            self.name, self.instructions, self.cap
        )
    }
}

fn fast_emit_fallback(
    module: &inkwell::module::Module<'_>,
    budget: FastEmitBudget,
) -> Option<FastEmitFallback> {
    let cap = match budget {
        FastEmitBudget::Off => return None,
        FastEmitBudget::Cap(cap) => cap,
    };
    let mut widest: Option<FastEmitFallback> = None;
    let mut function = module.get_first_function();
    while let Some(f) = function {
        if f.count_basic_blocks() > 0 {
            let instructions = function_instruction_count(f);
            if instructions > cap
                && widest
                    .as_ref()
                    .is_none_or(|current| instructions > current.instructions)
            {
                widest = Some(FastEmitFallback {
                    name: f.get_name().to_string_lossy().into_owned(),
                    instructions,
                    cap,
                });
            }
        }
        function = f.get_next_function();
    }
    widest
}

/// Instruction budget for ONE function after `rewrite-statepoints-for-gc`.
///
/// This is the measured backstop for the estimate that keeps relocation
/// fan-out out of LLVM's optimizer input (#8583). A function past it is sent
/// back to codegen for a shadow-frame spill and then compiled again at the
/// requested optimization level (#8679); it is never demoted or refused.
///
/// Calibrated between the two measured points of #8128 on the Next 16.3.0
/// production bundle: the largest post-rewrite function that finished
/// comfortably at `-Os` was ~413k instructions, and the one that ran more
/// than 65 CPU-minutes without finishing was ~2.1M. 1.5 Mi sits between them
/// with margin on both sides. `PERRY_LL_RS4GC_MAX_INSTRS=<n>` raises or
/// lowers it, `warn:<n>` only warns, and `0`/`off` disables the check.
const DEFAULT_RS4GC_MAX_INSTRS: usize = 1_572_864;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RewriteBudget {
    Off,
    Error(usize),
    Warn(usize),
}

/// One function that must be re-lowered onto a shadow frame before LLVM can
/// safely optimize its codegen unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Rs4gcBudgetCause {
    /// The constructed function is already large enough that RS4GC's own
    /// liveness/rewrite walk may not finish.  The estimate uses the roots and
    /// non-leaf call sites LLVM will actually see, rather than another source
    /// syntax approximation.
    PreRewrite {
        root_allocas: usize,
        safepoints: usize,
        estimated_relocations: usize,
    },
    /// RS4GC finished, but its relocation fan-out made the rewritten body too
    /// large for the normal optimization pipeline.
    PostRewrite { post_instructions: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rs4gcBudgetViolation {
    /// LLVM symbol of the function to spill.
    pub name: String,
    /// Instruction count before RS4GC, when the caller requested a census.
    pub pre_instructions: Option<usize>,
    /// The pre- or post-rewrite condition that requested the retry.
    pub cause: Rs4gcBudgetCause,
    /// Active limit for the cause's estimate.
    pub cap: usize,
}

/// Typed backend signal consumed by the codegen retry loops. Keeping this as
/// an error lets every existing LLVM API stop before the super-linear
/// optimizer, while the type (preserved through `anyhow` contexts) prevents
/// callers from scraping a diagnostic string for function names.
#[derive(Debug)]
struct Rs4gcBudgetExceeded {
    violations: Vec<Rs4gcBudgetViolation>,
}

impl std::fmt::Display for Rs4gcBudgetExceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (index, violation) in self.violations.iter().enumerate() {
            if index != 0 {
                writeln!(f)?;
            }
            write!(f, "{}", rewrite_budget_message(violation, true))?;
        }
        Ok(())
    }
}

impl std::error::Error for Rs4gcBudgetExceeded {}

/// Recover an RS4GC spill request through any diagnostic contexts added by
/// the native or text transport layers.
pub(crate) fn rs4gc_budget_retry(error: &anyhow::Error) -> Option<Vec<Rs4gcBudgetViolation>> {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<Rs4gcBudgetExceeded>())
        .map(|request| request.violations.clone())
}

fn parse_rewrite_budget(value: Option<&str>) -> RewriteBudget {
    match value.map(str::trim) {
        None | Some("") => RewriteBudget::Error(DEFAULT_RS4GC_MAX_INSTRS),
        Some("0") | Some("off") | Some("false") => RewriteBudget::Off,
        Some(v) => {
            if let Some(n) = v.strip_prefix("warn:") {
                match n.trim().parse::<usize>() {
                    Ok(0) => RewriteBudget::Off,
                    Ok(n) => RewriteBudget::Warn(n),
                    Err(_) => RewriteBudget::Warn(DEFAULT_RS4GC_MAX_INSTRS),
                }
            } else {
                match v.parse::<usize>() {
                    Ok(n) => RewriteBudget::Error(n),
                    Err(_) => RewriteBudget::Error(DEFAULT_RS4GC_MAX_INSTRS),
                }
            }
        }
    }
}

fn rs4gc_instruction_budget() -> RewriteBudget {
    #[cfg(test)]
    if let Some(budget) = TEST_RS4GC_BUDGET.with(std::cell::Cell::get) {
        return budget;
    }
    parse_rewrite_budget(std::env::var("PERRY_LL_RS4GC_MAX_INSTRS").ok().as_deref())
}

#[cfg(test)]
thread_local! {
    static TEST_RS4GC_BUDGET: std::cell::Cell<Option<RewriteBudget>> = const {
        std::cell::Cell::new(None)
    };
}

/// Thread-local budget seam for native-construction tests. Unlike mutating
/// `PERRY_LL_RS4GC_MAX_INSTRS`, this cannot make concurrently-running LLVM
/// tests spuriously spill or fail.
#[cfg(test)]
pub(crate) fn with_test_rs4gc_budget<T>(cap: usize, run: impl FnOnce() -> T) -> T {
    struct Restore(Option<RewriteBudget>);
    impl Drop for Restore {
        fn drop(&mut self) {
            TEST_RS4GC_BUDGET.set(self.0);
        }
    }
    let old = TEST_RS4GC_BUDGET.replace(Some(RewriteBudget::Error(cap)));
    let _restore = Restore(old);
    run()
}

#[cfg(test)]
/// Return the producer thread's test-only error budget for worker inheritance.
pub(crate) fn test_rs4gc_budget_cap() -> Option<usize> {
    TEST_RS4GC_BUDGET.with(|budget| match budget.get() {
        Some(RewriteBudget::Error(cap)) => Some(cap),
        _ => None,
    })
}

#[cfg(test)]
/// Install the producer's test budget around one worker-thread backend call.
pub(crate) fn with_inherited_test_rs4gc_budget<T>(
    cap: Option<usize>,
    run: impl FnOnce() -> T,
) -> T {
    match cap {
        Some(cap) => with_test_rs4gc_budget(cap, run),
        None => run(),
    }
}

/// Names of functions that actually entered RS4GC. A shadow-spilled function
/// still lives in a native-roots module, but carries no GC strategy and must
/// not trip the retry budget a second time merely because its ordinary body
/// is large.
fn rs4gc_functions(module: &inkwell::module::Module<'_>) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    let mut function = module.get_first_function();
    while let Some(f) = function {
        if f.count_basic_blocks() > 0 {
            let gc = unsafe { llvm_sys::core::LLVMGetGC(f.as_value_ref()) };
            if !gc.is_null()
                && unsafe { std::ffi::CStr::from_ptr(gc) }.to_bytes() == b"statepoint-example"
            {
                names.insert(f.get_name().to_string_lossy().into_owned());
            }
        }
        function = f.get_next_function();
    }
    names
}

/// The two constructed-IR factors that bound RS4GC relocation fan-out.
///
/// Count only allocas whose payload is a managed pointer and call sites which
/// are not explicitly marked as GC leaves. LLVM intrinsics are also leaves:
/// they cannot enter Perry's runtime or collect. This is deliberately the
/// same conservative model as the source-level spill estimate — each
/// safepoint can leave one additional pointer result live across later calls —
/// but it observes the calls codegen actually emitted. That closes estimator
/// holes where one source expression expands into several collecting helpers.
fn rs4gc_preflight_factors(function: inkwell::values::FunctionValue<'_>) -> (usize, usize) {
    let mut root_allocas = 0usize;
    let mut safepoints = 0usize;
    for bb in function.get_basic_blocks() {
        let mut inst = bb.get_first_instruction();
        while let Some(i) = inst {
            match i.get_opcode() {
                inkwell::values::InstructionOpcode::Alloca => {
                    if matches!(
                        i.get_allocated_type(),
                        Ok(inkwell::types::BasicTypeEnum::PointerType(ptr))
                            if ptr.get_address_space() == inkwell::AddressSpace::from(1u16)
                    ) {
                        root_allocas += 1;
                    }
                }
                inkwell::values::InstructionOpcode::Call
                | inkwell::values::InstructionOpcode::CallBr
                | inkwell::values::InstructionOpcode::Invoke => {
                    // Call, invoke and callbr are all LLVM CallBase values, so
                    // the call-site attribute API is valid for each opcode.
                    let call = unsafe { inkwell::values::CallSiteValue::new(i.as_value_ref()) };
                    let gc_leaf = call
                        .get_string_attribute(
                            inkwell::attributes::AttributeLoc::Function,
                            "gc-leaf-function",
                        )
                        .is_some();
                    let intrinsic = call
                        .get_called_fn_value()
                        .map_or(false, |callee| callee.get_intrinsic_id() != 0);
                    if !gc_leaf && !intrinsic {
                        safepoints += 1;
                    }
                }
                _ => {}
            }
            inst = i.get_next_instruction();
        }
    }
    (root_allocas, safepoints)
}

/// Every RS4GC-participating function whose constructed IR predicts more
/// relocation work than the source-level spill budget permits.
fn rs4gc_preflight_violations(
    module: &inkwell::module::Module<'_>,
    cap: usize,
    rewritten_functions: &std::collections::HashSet<String>,
) -> Vec<(String, usize, usize, usize)> {
    if cap == 0 {
        return Vec::new();
    }
    let mut over = Vec::new();
    let mut function = module.get_first_function();
    while let Some(f) = function {
        if f.count_basic_blocks() > 0 {
            let name = f.get_name().to_string_lossy().into_owned();
            if rewritten_functions.contains(&name) {
                let (root_allocas, safepoints) = rs4gc_preflight_factors(f);
                let live_roots =
                    crate::codegen::helpers::spill_live_root_count(root_allocas, safepoints);
                let estimate =
                    crate::codegen::helpers::root_relocation_estimate(live_roots, safepoints);
                if estimate > cap {
                    over.push((name, root_allocas, safepoints, estimate));
                }
            }
        }
        function = f.get_next_function();
    }
    over
}

/// Stop before RS4GC itself enters its super-linear liveness/rewrite walk and
/// ask codegen to re-lower the named functions with precise shadow roots.
fn enforce_rs4gc_preflight_budget(
    module: &inkwell::module::Module<'_>,
    cap: usize,
    pre: &std::collections::HashMap<String, usize>,
    rewritten_functions: &std::collections::HashSet<String>,
) -> Result<()> {
    let violations: Vec<Rs4gcBudgetViolation> =
        rs4gc_preflight_violations(module, cap, rewritten_functions)
            .into_iter()
            .map(
                |(name, root_allocas, safepoints, estimated_relocations)| Rs4gcBudgetViolation {
                    pre_instructions: pre.get(&name).copied(),
                    name,
                    cause: Rs4gcBudgetCause::PreRewrite {
                        root_allocas,
                        safepoints,
                        estimated_relocations,
                    },
                    cap,
                },
            )
            .collect();
    if violations.is_empty() {
        Ok(())
    } else {
        Err(anyhow::Error::new(Rs4gcBudgetExceeded { violations }))
    }
}

/// Every RS4GC-participating function whose post-rewrite body exceeds `cap`.
fn rs4gc_budget_violations(
    module: &inkwell::module::Module<'_>,
    cap: usize,
    rewritten_functions: &std::collections::HashSet<String>,
) -> Vec<(String, usize)> {
    let mut over = Vec::new();
    let mut function = module.get_first_function();
    while let Some(f) = function {
        if f.count_basic_blocks() > 0 {
            let name = f.get_name().to_string_lossy().into_owned();
            let n = function_instruction_count(f);
            if n > cap && rewritten_functions.contains(&name) {
                over.push((name, n));
            }
        }
        function = f.get_next_function();
    }
    over
}

fn rewrite_budget_message(violation: &Rs4gcBudgetViolation, retry: bool) -> String {
    let outcome = if retry {
        "Perry will re-lower this function with precise roots in a shadow frame, then retry the \
         unit at the requested optimization level"
    } else {
        "the warning-only budget override leaves the function for LLVM to optimize"
    };
    match &violation.cause {
        Rs4gcBudgetCause::PreRewrite {
            root_allocas,
            safepoints,
            estimated_relocations,
        } => format!(
            "before rewrite-statepoints-for-gc, `{}` has {root_allocas} managed-root allocas and \
             {safepoints} non-leaf call sites; accounting for call-result temporaries predicts \
             {estimated_relocations} relocations, above the pre-rewrite budget {}. RS4GC's own \
             liveness/rewrite walk is super-linear on fan-out of this size; {outcome} (#8583). \
             Override with PERRY_ROOT_SPILL_RELOCATIONS=<n> (raise) or =0 (disable).",
            violation.name, violation.cap
        ),
        Rs4gcBudgetCause::PostRewrite { post_instructions } => {
            let before = violation
                .pre_instructions
                .map(|n| format!(" (it was {n} before the rewrite)"))
                .unwrap_or_default();
            format!(
                "rewrite-statepoints-for-gc grew `{}` to {post_instructions} \
                 instructions{before}; the per-function budget is {}. LLVM's optimizer is \
                 super-linear on statepoint relocation fan-out of this size; {outcome} (#8679). \
                 Override with PERRY_LL_RS4GC_MAX_INSTRS=<n> (raise), =warn:<n> (warn only) or \
                 =0 (disable).",
                violation.name, violation.cap
            )
        }
    }
}

/// Apply [`RewriteBudget`] to a rewritten module. `pre` gives each function's
/// pre-rewrite size for the message, when the caller took a census.
fn enforce_rs4gc_instruction_budget(
    module: &inkwell::module::Module<'_>,
    budget: RewriteBudget,
    pre: &std::collections::HashMap<String, usize>,
    rewritten_functions: &std::collections::HashSet<String>,
) -> Result<()> {
    let (cap, fatal) = match budget {
        RewriteBudget::Off => return Ok(()),
        RewriteBudget::Error(cap) => (cap, true),
        RewriteBudget::Warn(cap) => (cap, false),
    };
    let over = rs4gc_budget_violations(module, cap, rewritten_functions);
    if over.is_empty() {
        return Ok(());
    }
    let violations: Vec<Rs4gcBudgetViolation> = over
        .into_iter()
        .map(|(name, post_instructions)| Rs4gcBudgetViolation {
            pre_instructions: pre.get(&name).copied(),
            name,
            cause: Rs4gcBudgetCause::PostRewrite { post_instructions },
            cap,
        })
        .collect();
    if fatal {
        return Err(anyhow::Error::new(Rs4gcBudgetExceeded { violations }));
    }
    for violation in &violations {
        eprintln!(
            "perry: warning: {}",
            rewrite_budget_message(violation, false)
        );
    }
    Ok(())
}

/// Per-function pre-rewrite sizes, for the budget message. Only the names
/// are retained, so this is a few bytes per function, not per instruction.
fn pre_rewrite_sizes(
    module: &inkwell::module::Module<'_>,
) -> std::collections::HashMap<String, usize> {
    let mut sizes = std::collections::HashMap::new();
    let mut function = module.get_first_function();
    while let Some(f) = function {
        if f.count_basic_blocks() > 0 {
            sizes.insert(
                f.get_name().to_string_lossy().into_owned(),
                function_instruction_count(f),
            );
        }
        function = f.get_next_function();
    }
    sizes
}

/// Per-function budget for TailCallElim's alloca-escape walk (#8883).
///
/// `TailCallElimPass::markTails` starts a use-def walk at EVERY alloca (and
/// byval argument) and follows the transitive SSA uses: through call
/// results, phis, selects, casts, GEPs and arithmetic; only a `load` or
/// `store` ends a branch, and only a `nocapture` call argument. In a
/// statepoint-rewritten function an alloca handed to any runtime call (the
/// argument arrays Perry builds on the stack) reaches the statepoint token,
/// every `gc.relocate` hanging off it, and through their `gc-live` bundles
/// every later statepoint — so each walk covers close to the whole function
/// and the pass costs `allocas × uses`, not `uses`. The reported Next.js
/// route (jsonwebtoken's bundled entry, 400 allocas, 643k post-RS4GC
/// instructions, 3.4k statepoints with 477k relocates) held one LLVM worker
/// for ~100 CPU-minutes in that walk, on a unit the rest of `-Os` finishes
/// in ~20 s.
///
/// The estimate is the product `allocas × instructions` of the function LLVM
/// is about to optimize — an upper bound on the walk that costs one linear
/// pass to compute. A function over the cap is stamped
/// `"disable-tail-calls"="true"`, which is the switch TRE itself honours
/// (`eliminateTailRecursion` returns before `markTails`). #8421's contract
/// — every function optimized at the requested level — is kept for every
/// other pass: the function still goes through the full `default<O*>`
/// pipeline. What it gives up is exactly what the attribute names: tail
/// recursion is not turned into a loop, and the backend does not emit calls
/// in return position as jumps (SelectionDAG's `canTailCall` and GlobalISel's
/// `CallLowering` both read the attribute; `musttail` is exempt and Perry
/// emits none). It is NOT `optnone` — #8583's RS4GC-root hazard does not
/// apply, because RS4GC has already run when the attribute is stamped and
/// nothing about GC roots changes.
///
/// `PERRY_LL_TRE_MAX_ALLOCA_WALK=<n>` raises or lowers the cap; `0`/`off`
/// disables the budget (every function keeps TRE, whatever it costs).
const DEFAULT_TRE_MAX_ALLOCA_WALK: u64 = 1 << 26;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TreWalkBudget {
    Off,
    Cap(u64),
}

fn parse_tre_walk_budget(value: Option<&str>) -> TreWalkBudget {
    match value.map(str::trim) {
        None | Some("") => TreWalkBudget::Cap(DEFAULT_TRE_MAX_ALLOCA_WALK),
        Some("0") | Some("off") | Some("false") => TreWalkBudget::Off,
        Some(v) => match v.parse::<u64>() {
            Ok(0) => TreWalkBudget::Off,
            Ok(n) => TreWalkBudget::Cap(n),
            Err(_) => TreWalkBudget::Cap(DEFAULT_TRE_MAX_ALLOCA_WALK),
        },
    }
}

fn tre_walk_budget() -> TreWalkBudget {
    #[cfg(test)]
    if let Some(budget) = TEST_TRE_WALK_BUDGET.with(std::cell::Cell::get) {
        return budget;
    }
    parse_tre_walk_budget(
        std::env::var("PERRY_LL_TRE_MAX_ALLOCA_WALK")
            .ok()
            .as_deref(),
    )
}

#[cfg(test)]
thread_local! {
    static TEST_TRE_WALK_BUDGET: std::cell::Cell<Option<TreWalkBudget>> = const {
        std::cell::Cell::new(None)
    };
}

/// Thread-local budget seam for tests, for the same reason as
/// [`with_test_rs4gc_budget`]: mutating `PERRY_LL_TRE_MAX_ALLOCA_WALK` would
/// race every concurrently running LLVM test in the binary.
#[cfg(test)]
pub(crate) fn with_test_tre_walk_budget<T>(cap: u64, run: impl FnOnce() -> T) -> T {
    struct Restore(Option<TreWalkBudget>);
    impl Drop for Restore {
        fn drop(&mut self) {
            TEST_TRE_WALK_BUDGET.with(|budget| budget.set(self.0));
        }
    }
    let old = TEST_TRE_WALK_BUDGET.replace(Some(TreWalkBudget::Cap(cap)));
    let _restore = Restore(old);
    run()
}

/// The function attribute TailCallElim and the backends' tail-call lowering
/// both read. Stamped by [`disable_tail_call_elim_over_budget`].
const DISABLE_TAIL_CALLS_ATTR: &str = "disable-tail-calls";

/// One function whose alloca-walk estimate exceeded the budget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreWalkOverBudget {
    pub name: String,
    pub allocas: usize,
    pub instructions: usize,
    pub cap: u64,
}

impl TreWalkOverBudget {
    fn estimate(&self) -> u64 {
        self.allocas as u64 * self.instructions as u64
    }
}

impl std::fmt::Display for TreWalkOverBudget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "`{}` has {} allocas across {} instructions (alloca-walk estimate {}, budget {}); \
             skipping tail-call elimination for it, because TailCallElim's alloca-escape walk \
             is quadratic in exactly that product on a statepoint-rewritten body (#8883). Every \
             other pass still runs at the requested level; the function only loses \
             tail-recursion-to-loop and sibling-call codegen. Override with \
             PERRY_LL_TRE_MAX_ALLOCA_WALK=<n> (raise) or =0 (disable).",
            self.name,
            self.allocas,
            self.instructions,
            self.estimate(),
            self.cap
        )
    }
}

/// `(allocas, instructions)` of one defined function — the two factors of
/// the walk estimate, from the same linear pass `function_instruction_count`
/// makes.
fn alloca_walk_factors(function: inkwell::values::FunctionValue<'_>) -> (usize, usize) {
    let mut allocas = 0usize;
    let mut instrs = 0usize;
    for bb in function.get_basic_blocks() {
        let mut inst = bb.get_first_instruction();
        while let Some(i) = inst {
            instrs += 1;
            if i.get_opcode() == inkwell::values::InstructionOpcode::Alloca {
                allocas += 1;
            }
            inst = i.get_next_instruction();
        }
    }
    (allocas, instrs)
}

/// Stamp `"disable-tail-calls"="true"` on every defined function whose
/// `allocas × instructions` exceeds `budget`, and return what was stamped
/// so the caller can say so. Runs on the module exactly as the optimization
/// pipeline will see it (after RS4GC under native roots).
fn disable_tail_call_elim_over_budget<'ctx>(
    module: &inkwell::module::Module<'ctx>,
    budget: TreWalkBudget,
) -> Vec<TreWalkOverBudget> {
    let cap = match budget {
        TreWalkBudget::Off => return Vec::new(),
        TreWalkBudget::Cap(cap) => cap,
    };
    let context = module.get_context();
    let mut over = Vec::new();
    let mut function = module.get_first_function();
    while let Some(f) = function {
        if f.count_basic_blocks() > 0 {
            let (allocas, instructions) = alloca_walk_factors(f);
            if allocas as u64 * instructions as u64 > cap {
                f.add_attribute(
                    inkwell::attributes::AttributeLoc::Function,
                    context.create_string_attribute(DISABLE_TAIL_CALLS_ATTR, "true"),
                );
                over.push(TreWalkOverBudget {
                    name: f.get_name().to_string_lossy().into_owned(),
                    allocas,
                    instructions,
                    cap,
                });
            }
        }
        function = f.get_next_function();
    }
    over
}
