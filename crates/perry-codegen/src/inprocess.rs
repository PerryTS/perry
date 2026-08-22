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

use std::ffi::CString;
use std::sync::Once;

use anyhow::{anyhow, Result};
use inkwell::context::Context;
use inkwell::memory_buffer::MemoryBuffer;
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine, TargetTriple,
};
use inkwell::OptimizationLevel;

/// The pass string that inserts every statepoint, relocation and
/// downstream-use rewrite — i.e. the whole native-roots lowering, after
/// codegen has retyped its root allocas to `ptr addrspace(1)`.
///
/// Named rather than spelled inline because `native_root_coverage` (#7502)
/// runs it too, and a coverage suite that spelled its own pass list would keep
/// passing against a pipeline production had stopped using. `mem2reg` is not
/// incidental company: RS4GC tracks `addrspace(1)` **SSA values**, not memory,
/// so a root alloca that survives promotion is a root the collector never sees.
// SCCP—not InstCombine—is before RS4GC deliberately (#8065). Native C-API construction
// folds constants as instructions are built, while whole-module text parsing
// retains the equivalent instruction graph. If RS4GC sees those two shapes
// before canonicalization, their live-root ordering can differ and reach both
// machine code and the compact GC map. The ordinary optimization pipeline is
// too late: statepoints and relocations have already been assigned by then.
// The narrower SCCP preserves dynamic pointer round trips which InstCombine
// can erase, so the positive live-root witness remains visible to RS4GC.
pub(crate) const STATEPOINT_REWRITE_PASSES: &str =
    "function(mem2reg,sccp),rewrite-statepoints-for-gc";

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
fn statepoint_rewritten_ir_with_passes(
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
/// after RS4GC, widest functions, phase times) for the per-unit report.
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

/// Instruction budget for ONE function after `rewrite-statepoints-for-gc`.
///
/// This is an assertion about the estimate that keeps relocation fan-out out
/// of LLVM's input (#8583), not an optimization policy: a function past it is
/// refused loudly, never demoted. The #8421 contract — every function is
/// optimized at the level the plan asked for — stays intact; what this adds
/// is that an estimator miss fails in seconds with the function's name and
/// sizes instead of hanging the build for hours.
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
    parse_rewrite_budget(std::env::var("PERRY_LL_RS4GC_MAX_INSTRS").ok().as_deref())
}

/// Every defined function whose post-rewrite body exceeds `cap`.
fn rs4gc_budget_violations(
    module: &inkwell::module::Module<'_>,
    cap: usize,
) -> Vec<(String, usize)> {
    let mut over = Vec::new();
    let mut function = module.get_first_function();
    while let Some(f) = function {
        if f.count_basic_blocks() > 0 {
            let n = function_instruction_count(f);
            if n > cap {
                over.push((f.get_name().to_string_lossy().into_owned(), n));
            }
        }
        function = f.get_next_function();
    }
    over
}

fn rewrite_budget_message(name: &str, post: usize, cap: usize, pre: Option<usize>) -> String {
    let before = pre
        .map(|n| format!(" (it was {n} before the rewrite)"))
        .unwrap_or_default();
    format!(
        "rewrite-statepoints-for-gc grew `{name}` to {post} instructions{before}; the \
         per-function budget is {cap}. LLVM's optimizer is super-linear on statepoint \
         relocation fan-out of this size and the compile would not finish in practical \
         time, so the unit is refused instead of being left to hang. Perry does not lower \
         the optimization level for it: the fix is to keep this function's GC roots out \
         of the relocation set or to split it (#8583). Override with \
         PERRY_LL_RS4GC_MAX_INSTRS=<n> (raise), =warn:<n> (warn only) or =0 (disable)."
    )
}

/// Apply [`RewriteBudget`] to a rewritten module. `pre` gives each function's
/// pre-rewrite size for the message, when the caller took a census.
fn enforce_rs4gc_instruction_budget(
    module: &inkwell::module::Module<'_>,
    budget: RewriteBudget,
    pre: &std::collections::HashMap<String, usize>,
) -> Result<()> {
    let (cap, fatal) = match budget {
        RewriteBudget::Off => return Ok(()),
        RewriteBudget::Error(cap) => (cap, true),
        RewriteBudget::Warn(cap) => (cap, false),
    };
    let over = rs4gc_budget_violations(module, cap);
    if over.is_empty() {
        return Ok(());
    }
    let messages: Vec<String> = over
        .iter()
        .map(|(name, post)| rewrite_budget_message(name, *post, cap, pre.get(name).copied()))
        .collect();
    if fatal {
        return Err(anyhow!("{}", messages.join("\n")));
    }
    for m in messages {
        eprintln!("perry: warning: {m}");
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

fn optimize_and_emit(
    module: &inkwell::module::Module<'_>,
    effective_target: &str,
    opt: char,
    mcpu_native: bool,
    explicit_cpu: Option<&str>,
    mllvm: &[String],
    emit_asm: bool,
    native_roots: bool,
    mut stats: Option<&mut UnitCodegenStats>,
) -> Result<Vec<u8>> {
    global_init(mllvm);
    announce();

    module
        .verify()
        .map_err(|e| anyhow!("LLVM verifier rejected module:\n{}", e.to_string()))?;

    let triple = TargetTriple::create(effective_target);
    let target = Target::from_triple(&triple)
        .map_err(|e| anyhow!("no LLVM target for `{effective_target}`: {e}"))?;
    let (cpu, features) = if mcpu_native {
        (
            TargetMachine::get_host_cpu_name()
                .to_string_lossy()
                .into_owned(),
            TargetMachine::get_host_cpu_features()
                .to_string_lossy()
                .into_owned(),
        )
    } else if let Some(cpu) = explicit_cpu {
        (cpu.to_string(), String::new())
    } else {
        (
            default_cpu_for_triple(effective_target).to_string(),
            String::new(),
        )
    };
    let opt_level = match opt {
        '0' => OptimizationLevel::None,
        '1' => OptimizationLevel::Less,
        '2' | 's' | 'z' => OptimizationLevel::Default,
        _ => OptimizationLevel::Aggressive,
    };
    let tm = target
        .create_target_machine(
            &triple,
            &cpu,
            &features,
            opt_level,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or_else(|| anyhow!("failed to create TargetMachine for `{effective_target}`"))?;

    // Same trust order as the subprocess path: `-target` wins over whatever
    // triple the module text states, and the module optimizes under the
    // machine's real datalayout.
    module.set_triple(&triple);
    module.set_data_layout(&tm.get_target_data().get_data_layout());

    // RS4GC must run BEFORE the optimization pipeline, and — critically — in
    // this process, against this LLVM.
    //
    // The external path shells `rewrite-statepoints-for-gc` out to an `opt`
    // binary and then hands the rewritten IR to `clang -c`. When those are
    // different LLVM versions (Homebrew 22 and Apple clang 21 is the ordinary
    // macOS case) the emitted IR uses constructs the older parser rejects, and
    // the compile dies with `error: unterminated attribute group`. That is why
    // RS4GC needed `PERRY_LLVM_CLANG` pointed at a version-matched toolchain,
    // and why it did not work on a stock install at all.
    //
    // Here the same `TargetMachine` runs the pass and emits the object, so the
    // skew cannot exist. This matters beyond convenience: RS4GC is the only
    // backend that can root an `invoke`, and since #7302 every call inside a
    // `try` is one — 26% of the gap suite (128 of 479 files) contains a `try`,
    // which the explicit bridge refuses outright (#7327/#7330).
    if native_roots {
        // Sizes before the rewrite: the budget message below names them, and
        // the per-unit report compares them with the post-rewrite census.
        let budget = rs4gc_instruction_budget();
        let pre_sizes = if budget == RewriteBudget::Off && stats.is_none() {
            std::collections::HashMap::new()
        } else {
            pre_rewrite_sizes(module)
        };
        if let Some(stats) = stats.as_deref_mut() {
            stats.functions = pre_sizes.len();
            stats.pre_rewrite_instructions = pre_sizes.values().sum();
            stats.pre_rewrite_widest = pre_sizes
                .iter()
                .max_by_key(|(_, n)| **n)
                .map(|(name, n)| (name.clone(), *n));
        }
        let rewrite_started = std::time::Instant::now();
        module
            .run_passes(STATEPOINT_REWRITE_PASSES, &tm, PassBuilderOptions::create())
            .map_err(|e| {
                anyhow!(
                    "in-process rewrite-statepoints-for-gc failed:\n{}",
                    e.to_string()
                )
            })?;
        // Verify the rewritten module before it reaches the backend. RS4GC
        // has produced verifier-invalid IR in the wild (#8121: it wrapped an
        // inline-asm barrier into a gc.statepoint), and unlike the external
        // `opt` path — whose verifier aborts with the broken instruction —
        // the in-process pipeline would feed the broken module straight to
        // ISel, where it dies as a bare SIGBUS with no diagnostic.
        module.verify().map_err(|e| {
            anyhow!(
                "in-process rewrite-statepoints-for-gc produced a module the \
                 verifier rejects (this is a Perry codegen bug — the input \
                 shape must be exempted or fixed):\n{}",
                e.to_string()
            )
        })?;
        if let Some(stats) = stats.as_deref_mut() {
            stats.rewrite_secs = rewrite_started.elapsed().as_secs_f64();
            let (_, total, widest) = module_instruction_census(module);
            stats.post_rewrite_instructions = total;
            stats.post_rewrite_widest = widest;
        }
        // The relocation-fan-out assertion (#8583): refuse, never demote.
        enforce_rs4gc_instruction_budget(module, budget, &pre_sizes)?;
    }

    let pipeline = match opt {
        '0' => "default<O0>",
        '1' => "default<O1>",
        '2' => "default<O2>",
        's' => "default<Os>",
        'z' => "default<Oz>",
        _ => "default<O3>",
    };
    let optimize_started = std::time::Instant::now();
    module
        .run_passes(pipeline, &tm, PassBuilderOptions::create())
        .map_err(|e| anyhow!("pass pipeline `{pipeline}` failed:\n{}", e.to_string()))?;
    if let Some(stats) = stats.as_deref_mut() {
        stats.optimize_secs = optimize_started.elapsed().as_secs_f64();
    }

    let kind = if emit_asm {
        FileType::Assembly
    } else {
        FileType::Object
    };
    let emit_started = std::time::Instant::now();
    let obj = tm
        .write_to_memory_buffer(module, kind)
        .map_err(|e| anyhow!("{kind:?} emission failed:\n{}", e.to_string()))?;
    if let Some(stats) = stats {
        stats.emit_secs = emit_started.elapsed().as_secs_f64();
    }
    Ok(obj.as_slice().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asm_barrier_fixture(leaf_attr: &str) -> String {
        format!(
            "declare i64 @may_collect()\n\n\
             define i64 @f(i64 %a) gc \"statepoint-example\" {{\n\
             entry:\n\
             \x20 %slot = alloca ptr addrspace(1)\n\
             \x20 %p = inttoptr i64 %a to ptr addrspace(1)\n\
             \x20 store ptr addrspace(1) %p, ptr %slot\n\
             \x20 call void asm sideeffect \"\", \"\"(){leaf_attr}\n\
             \x20 %t = call i64 @may_collect()\n\
             \x20 %after = load ptr addrspace(1), ptr %slot\n\
             \x20 %bits = ptrtoint ptr addrspace(1) %after to i64\n\
             \x20 %r = add i64 %t, %bits\n\
             \x20 ret i64 %r\n\
             }}\n"
        )
    }

    #[test]
    fn gc_leaf_asm_barrier_survives_rs4gc_unwrapped() {
        // The shipped emitters stamp the loop-preservation barrier
        // `"gc-leaf-function"`; RS4GC must leave it as a plain inline-asm
        // call while still statepointing the real call next to it.
        let rewritten = statepoint_rewritten_ir(
            &asm_barrier_fixture(" \"gc-leaf-function\""),
            "arm64-apple-darwin",
            "asm_barrier_leaf",
        )
        .expect("attributed barrier must survive the rewrite");
        assert!(
            rewritten.contains("call void asm sideeffect"),
            "barrier must remain a plain inline-asm call:\n{rewritten}"
        );
        assert!(
            !rewritten.contains("elementtype(void ()) asm"),
            "barrier must not be statepoint-wrapped:\n{rewritten}"
        );
        assert!(
            rewritten.contains("@llvm.experimental.gc.statepoint"),
            "the genuine call must still be statepointed:\n{rewritten}"
        );
    }

    #[test]
    fn unattributed_asm_barrier_is_rejected_not_miscompiled() {
        // Sabotage arm: without the attribute RS4GC wraps the asm into a
        // gc.statepoint whose callee is inline asm — invalid IR. The
        // pipeline must fail verification loudly (#8121's SIGBUS shape),
        // proving the leaf test above can actually fail.
        let result = statepoint_rewritten_ir(
            &asm_barrier_fixture(""),
            "arm64-apple-darwin",
            "asm_barrier_broken",
        );
        assert!(
            result.is_err(),
            "an unattributed barrier must be rejected by the verifier"
        );
    }

    #[test]
    fn rewrite_budget_spellings() {
        assert_eq!(
            parse_rewrite_budget(None),
            RewriteBudget::Error(DEFAULT_RS4GC_MAX_INSTRS)
        );
        assert_eq!(parse_rewrite_budget(Some("0")), RewriteBudget::Off);
        assert_eq!(parse_rewrite_budget(Some("off")), RewriteBudget::Off);
        assert_eq!(
            parse_rewrite_budget(Some(" 250000 ")),
            RewriteBudget::Error(250_000)
        );
        assert_eq!(
            parse_rewrite_budget(Some("warn:4096")),
            RewriteBudget::Warn(4096)
        );
        assert_eq!(parse_rewrite_budget(Some("warn:0")), RewriteBudget::Off);
        // Unparsable values keep the default rather than silently disabling.
        assert_eq!(
            parse_rewrite_budget(Some("lots")),
            RewriteBudget::Error(DEFAULT_RS4GC_MAX_INSTRS)
        );
    }

    /// Six gc values live across forty safepoints: ~60 instructions before
    /// `rewrite-statepoints-for-gc`, a few hundred after (each statepoint
    /// relocates every live value). A budget between the two is exceeded
    /// only by the post-rewrite module — which is the property the
    /// assertion exists for. Counting BEFORE the rewrite (the #8421
    /// replacement knob's mistake) would make `after` empty and fail here.
    fn relocation_fanout_fixture() -> String {
        let mut ir = String::from(
            "declare i64 @may_collect()\n\n\
             define i64 @f(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5) gc \"statepoint-example\" {\n\
             entry:\n",
        );
        for i in 0..6 {
            ir.push_str(&format!(
                "  %p{i} = inttoptr i64 %a{i} to ptr addrspace(1)\n"
            ));
        }
        for c in 0..40 {
            ir.push_str(&format!("  %c{c} = call i64 @may_collect()\n"));
        }
        for i in 0..6 {
            ir.push_str(&format!(
                "  %b{i} = ptrtoint ptr addrspace(1) %p{i} to i64\n"
            ));
        }
        ir.push_str(
            "  %s0 = add i64 %b0, %b1\n  %s1 = add i64 %s0, %b2\n  %s2 = add i64 %s1, %b3\n\
             \x20 %s3 = add i64 %s2, %b4\n  %s4 = add i64 %s3, %b5\n  %s5 = add i64 %s4, %c0\n\
             \x20 %s6 = add i64 %s5, %c39\n  ret i64 %s6\n}\n",
        );
        ir
    }

    #[test]
    fn rs4gc_budget_fires_only_on_the_rewritten_module() {
        global_init(&[]);
        let target = "arm64-apple-darwin";
        let fixture = relocation_fanout_fixture();
        let rewritten = statepoint_rewritten_ir(&fixture, target, "fanout_budget")
            .expect("fan-out fixture must run RS4GC");

        let context = Context::create();
        let before = parse_ir_text(&context, &fixture, "fanout_before").expect("fixture parses");
        let after = parse_ir_text(&context, &rewritten, "fanout_after").expect("rewritten parses");
        let pre = pre_rewrite_sizes(&before);
        let pre_f = pre["f"];
        let (_, post_total, post_widest) = module_instruction_census(&after);
        let post_f = post_widest.as_ref().map(|(_, n)| *n).unwrap_or(0);
        assert!(
            post_f > 3 * pre_f,
            "fixture must grow under relocation fan-out (pre {pre_f}, post {post_f}):\n{rewritten}"
        );
        assert_eq!(post_total, post_f, "one defined function");
        let cap = pre_f + (post_f - pre_f) / 2;

        assert!(
            rs4gc_budget_violations(&before, cap).is_empty(),
            "the pre-rewrite module is under the budget by construction"
        );
        let over = rs4gc_budget_violations(&after, cap);
        assert_eq!(
            over.len(),
            1,
            "exactly the rewritten body is over: {over:?}"
        );
        assert_eq!(over[0].0, "f");
        assert_eq!(over[0].1, post_f);

        let err = enforce_rs4gc_instruction_budget(&after, RewriteBudget::Error(cap), &pre)
            .expect_err("the default spelling refuses the unit");
        let msg = format!("{err:#}");
        for needle in [
            "`f`",
            &format!("to {post_f} instructions"),
            &format!("it was {pre_f} before"),
            &format!("budget is {cap}"),
            "PERRY_LL_RS4GC_MAX_INSTRS",
            "#8583",
        ] {
            assert!(
                msg.contains(needle),
                "message must carry {needle:?}:\n{msg}"
            );
        }
        assert!(
            !msg.contains("optnone"),
            "the budget is an assertion, never a demotion:\n{msg}"
        );
        enforce_rs4gc_instruction_budget(&after, RewriteBudget::Warn(cap), &pre)
            .expect("warn spelling does not refuse");
        enforce_rs4gc_instruction_budget(&after, RewriteBudget::Off, &pre)
            .expect("off spelling does not refuse");
        enforce_rs4gc_instruction_budget(&after, RewriteBudget::Error(post_f), &pre)
            .expect("a budget at the exact size is not exceeded");
    }

    fn constant_fold_order_fixture(folded: bool) -> String {
        let mut ir = String::from(
            "declare i64 @may_collect()\n\ndefine i64 @f(i64 %d0, i64 %d1, i64 %d2, i64 %d3, i64 %d4, i64 %d5, i64 %d6, i64 %d7) gc \"statepoint-example\" {\nentry:\n",
        );
        for i in 0..8 {
            ir.push_str(&format!("  %cslot{i} = alloca ptr addrspace(1)\n"));
            if folded {
                ir.push_str(&format!(
                    "  store ptr addrspace(1) inttoptr (i64 9222246136947933185 to ptr addrspace(1)), ptr %cslot{i}\n"
                ));
            } else {
                ir.push_str(&format!(
                    "  %cb{i} = bitcast double 0x7FFC000000000001 to i64\n  %cp{i} = inttoptr i64 %cb{i} to ptr addrspace(1)\n  store ptr addrspace(1) %cp{i}, ptr %cslot{i}\n"
                ));
            }
        }
        for i in 0..8 {
            ir.push_str(&format!(
                "  %dslot{i} = alloca ptr addrspace(1)\n  %dp{i} = inttoptr i64 %d{i} to ptr addrspace(1)\n  store ptr addrspace(1) %dp{i}, ptr %dslot{i}\n"
            ));
        }
        ir.push_str("  %sp = call i64 @may_collect()\n");
        for i in 0..8 {
            ir.push_str(&format!(
                "  %after{i} = load ptr addrspace(1), ptr %dslot{i}\n  %bits{i} = ptrtoint ptr addrspace(1) %after{i} to i64\n"
            ));
        }
        for i in 0..8 {
            ir.push_str(&format!(
                "  %cafter{i} = load ptr addrspace(1), ptr %cslot{i}\n  %cbits{i} = ptrtoint ptr addrspace(1) %cafter{i} to i64\n"
            ));
        }
        ir.push_str("  %x1 = xor i64 %bits0, %bits1\n");
        for i in 2..8 {
            ir.push_str(&format!("  %x{i} = xor i64 %x{}, %bits{i}\n", i - 1));
        }
        ir.push_str("  %y0 = xor i64 %x7, %cbits0\n");
        for i in 1..8 {
            ir.push_str(&format!("  %y{i} = xor i64 %y{}, %cbits{i}\n", i - 1));
        }
        ir.push_str("  ret i64 %y7\n}\n");
        ir
    }

    #[test]
    fn rs4gc_canonicalizes_construction_time_folds_before_root_liveness() {
        let _native = crate::codegen::helpers::NativeRootsPin::native();
        let target = crate::codegen::default_target_triple();
        let text_ir = constant_fold_order_fixture(false);
        let folded_ir = constant_fold_order_fixture(true);

        for (label, ir) in [("text", &text_ir), ("folded", &folded_ir)] {
            let rewritten = statepoint_rewritten_ir(ir, &target, label)
                .unwrap_or_else(|e| panic!("{label} fixture must run RS4GC: {e:#}"));
            assert!(
                !rewritten.contains("%cb0 = bitcast"),
                "{label} fixture reached RS4GC before construction-time folds converged:\n{rewritten}"
            );
            let live_bundle = rewritten
                .lines()
                .find(|line| line.contains("\"gc-live\""))
                .unwrap_or_else(|| panic!("{label} fixture lost every dynamic root:\n{rewritten}"));
            assert!(
                live_bundle.contains("%dp0"),
                "{label} fixture lost every dynamic root:\n{rewritten}"
            );
            assert!(
                rewritten.contains("gc.relocate"),
                "{label} fixture did not relocate a dynamic root:\n{rewritten}"
            );
        }

        let emit = |ir: &str, name: &str| {
            let context = Context::create();
            let module = parse_ir_text(&context, ir, name).expect("fixture parses");
            optimize_and_emit_module(&module, &target, &["-O3".into(), "-S".into()], true)
                .expect("fixture emits assembly")
        };
        // Both arms must be emitted under the SAME module name. The name
        // becomes the module id, and on ELF the assembler writes it into the
        // object as a `.file` directive — so two differently-named arms differ
        // by that one line no matter how perfectly the code itself converged.
        // Mach-O records no such directive, which is why naming them apart only
        // ever failed on Linux (#8087).
        let text = emit(&text_ir, "constant_fold_order");
        let folded = emit(&folded_ir, "constant_fold_order");
        assert_eq!(
            text, folded,
            "construction-time constant folding must converge before RS4GC assigns root liveness"
        );

        const PRE_FIX_PASSES: &str = "function(mem2reg),rewrite-statepoints-for-gc";
        let pre_fix_emit = |ir: &str, name: &str| {
            let rewritten = statepoint_rewritten_ir_with_passes(
                ir,
                &target,
                &format!("{name}_rewrite"),
                PRE_FIX_PASSES,
            )
            .expect("pre-fix pipeline rewrites fixture");
            let context = Context::create();
            let module =
                parse_ir_text(&context, &rewritten, name).expect("rewritten fixture parses");
            let _shadow = crate::codegen::helpers::NativeRootsPin::shadow();
            (
                rewritten,
                optimize_and_emit_module(&module, &target, &["-O3".into(), "-S".into()], false)
                    .expect("rewritten fixture emits assembly"),
            )
        };
        let (pre_fix_text_ir, pre_fix_text) = pre_fix_emit(&text_ir, "pre_fix_text");
        let (_, pre_fix_folded) = pre_fix_emit(&folded_ir, "pre_fix_native");
        assert!(
            pre_fix_text_ir
                .lines()
                .find(|line| line.contains("\"gc-live\""))
                .is_some_and(|line| line.contains("%cp0")),
            "negative control must keep a constant-derived text root live across the safepoint:\n{pre_fix_text_ir}"
        );
        assert_ne!(
            pre_fix_text, pre_fix_folded,
            "fixture must fail byte equality under the pre-#8065 pass order"
        );
    }

    /// Layer-2 readiness (#7174, engine-plan layer 0 -> 2): the in-process
    /// pipeline can schedule `RewriteStatepointsForGC` at the pinned LLVM —
    /// no `opt` subprocess, no version-skewed toolchain. This is the exact
    /// mechanism #7108 measured as viable-but-blocked under text-plus-clang.
    /// A statepoint lands at the may-GC call and the live GC pointer is
    /// relocated across it — the property that makes the register-held-
    /// pointer bug class unrepresentable.
    #[test]
    fn rs4gc_schedules_in_process() {
        let context = Context::create();
        let ir = r#"
declare ptr addrspace(1) @alloc()

define ptr addrspace(1) @f(ptr addrspace(1) %p) gc "statepoint-example" {
entry:
  %q = call ptr addrspace(1) @alloc()
  ret ptr addrspace(1) %p
}
"#;
        let module = parse_ir_text(&context, ir, "rs4gc_probe").expect("probe parses");
        global_init(&[]);
        let triple = TargetMachine::get_default_triple();
        let target = Target::from_triple(&triple).expect("host target");
        let tm = target
            .create_target_machine(
                &triple,
                "",
                "",
                OptimizationLevel::None,
                RelocMode::PIC,
                CodeModel::Default,
            )
            .expect("target machine");
        module
            .run_passes(
                "rewrite-statepoints-for-gc",
                &tm,
                PassBuilderOptions::create(),
            )
            .expect("RS4GC pipeline runs in-process");
        let printed = module.print_to_string().to_string();
        assert!(
            printed.contains("gc.statepoint"),
            "no statepoint emitted:\n{printed}"
        );
        assert!(
            printed.contains("gc.relocate"),
            "live GC pointer not relocated across the call:\n{printed}"
        );
        module.verify().expect("statepoint IR verifies");
    }

    /// The initialized backend set must cover every triple the compile driver
    /// can produce. `initialize_all()` cost +86.9 MB of static link for ~18
    /// unreachable backends; this pins the replacement, so narrowing it
    /// further — or adding a target without initializing its backend — fails
    /// here rather than at a user's compile.
    #[test]
    fn every_supported_triple_resolves_to_an_initialized_backend() {
        global_init(&[]);
        for triple in [
            "arm64-apple-macosx",
            "aarch64-apple-ios",
            "aarch64-apple-watchos",
            "arm64_32-apple-watchos",
            "aarch64-unknown-linux-gnu",
            "aarch64-unknown-linux-musl",
            "aarch64-linux-android",
            "x86_64-apple-darwin",
            "x86_64-unknown-linux-gnu",
            "x86_64-pc-windows-msvc",
            "i686-unknown-linux-gnu",
        ] {
            let t = TargetTriple::create(triple);
            assert!(
                Target::from_triple(&t).is_ok(),
                "{triple} has no initialized LLVM backend — the compile driver \
                 can emit this triple, so `global_init` must initialize it"
            );
        }
    }

    /// #7327 CI regression: an empty CPU string makes LLVM pick `generic`,
    /// which on aarch64 is ARMv8.0 and has no FEAT_JSCVT — so the
    /// `llvm.aarch64.fjcvtzs` that codegen emits for any Apple arm64 triple
    /// cannot be selected and the compile aborts. Clang defaults that triple to
    /// `apple-m1`, which is the assumption `set_jscvt_for_target` already makes.
    /// Reproduced with `PERRY_TARGET_CPU=generic`, which is the path CI took.
    #[test]
    fn apple_aarch64_defaults_to_a_cpu_with_feat_jscvt() {
        for triple in [
            "arm64-apple-macosx",
            "arm64-apple-darwin",
            "aarch64-apple-darwin",
            "arm64-apple-ios",
        ] {
            assert_eq!(
                default_cpu_for_triple(triple),
                "apple-m1",
                "{triple} must not fall back to LLVM's ARMv8.0 `generic`: codegen \
                 emits llvm.aarch64.fjcvtzs for Apple arm64 triples"
            );
        }
        // Everything else keeps LLVM's portable baseline, matching the clang
        // path when no tuning flag is passed.
        for triple in [
            "x86_64-apple-darwin",
            "aarch64-unknown-linux-gnu",
            "x86_64-unknown-linux-gnu",
        ] {
            assert_eq!(default_cpu_for_triple(triple), "", "{triple}");
        }
    }

    /// `-S` used to be swallowed by the catch-all that ignores `-c`, so the
    /// statepoint backends asked for assembly and were handed an object. The
    /// failure was invisible here and surfaced two steps later as
    /// `ld: unknown file type`, because #7314's compact-map rewriter rewrites
    /// `.llvm_stackmaps` in assembly *text* and had nothing to rewrite.
    #[test]
    fn dash_s_requests_assembly_and_dash_c_does_not() {
        let (_, _, _, _, emit_asm) =
            interpret_plan_args(&["-O2".into(), "-S".into()]).expect("args parse");
        assert!(emit_asm, "-S must request assembly");

        let (_, _, _, _, emit_asm) =
            interpret_plan_args(&["-O2".into(), "-c".into()]).expect("args parse");
        assert!(!emit_asm, "-c must still request an object");
    }

    /// The property the wiring depends on: the same module emitted with
    /// `FileType::Assembly` is assembler text carrying a stack-map section,
    /// not an object. If this ever silently produced an object again, the
    /// compact-map rewrite would find no `.llvm_stackmaps` to shrink and the
    /// GC would be reading an empty map — the #7332 shape, a binary that
    /// looks correct until a collection frees something live.
    #[test]
    fn assembly_emission_is_text_not_an_object() {
        let context = Context::create();
        let ir = r#"
define i32 @f(i32 %x) {
entry:
  %y = add i32 %x, 1
  ret i32 %y
}
"#;
        let module = parse_ir_text(&context, ir, "asm_probe").expect("probe parses");
        global_init(&[]);
        let triple = TargetMachine::get_default_triple();
        let target = Target::from_triple(&triple).expect("host target");
        let tm = target
            .create_target_machine(
                &triple,
                "",
                "",
                OptimizationLevel::None,
                RelocMode::PIC,
                CodeModel::Default,
            )
            .expect("target machine");

        let asm = tm
            .write_to_memory_buffer(&module, FileType::Assembly)
            .expect("assembly emission");
        let text = String::from_utf8_lossy(asm.as_slice()).to_string();
        assert!(
            text.contains(".globl") || text.contains(".global"),
            "expected assembler directives, got:\n{}",
            &text[..text.len().min(200)]
        );

        let obj = tm
            .write_to_memory_buffer(&module, FileType::Object)
            .expect("object emission");
        assert_ne!(
            asm.as_slice(),
            obj.as_slice(),
            "assembly and object emission returned identical bytes — `-S` is \
             being ignored somewhere in the emission path"
        );
    }
}
