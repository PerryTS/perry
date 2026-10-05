//! In-process `.ll -> .o` compilation through the LLVM C API (exp/llvm-inprocess).
//!
//! Built with `llvm-inprocess` and mandatory for targets with native stack maps.
//! Unsupported platforms may select the external compiler. Native roots are
//! published after optimization and emitted through LLVM statepoints here.
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

mod native_homes;
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
        // Standalone WASI (#11375): only with the off-by-default `target-wasi`
        // feature, so the shipped compiler does not carry the backend.
        #[cfg(feature = "target-wasi")]
        Target::initialize_webassembly(&cfg);
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
///
/// Returns one object (or assembly, under `-S`) per emitted piece: one
/// normally, two when the fast-emit budget split its offenders out (#10586).
/// `linker::finish_native_pieces` turns them into the single linker input.
pub fn compile_ll_to_object_inprocess(
    ll_text: &str,
    effective_target: &str,
    clang_style_args: &[String],
    module_name: &str,
    native_roots: bool,
) -> Result<Vec<Vec<u8>>> {
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
/// The native construction path calls this directly. Pieces as for
/// [`compile_ll_to_object_inprocess`]; the module is edited in place when the
/// fast-emit budget splits it.
pub(crate) fn optimize_and_emit_module(
    module: &inkwell::module::Module<'_>,
    effective_target: &str,
    clang_style_args: &[String],
    native_roots: bool,
) -> Result<Vec<Vec<u8>>> {
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
) -> Result<Vec<Vec<u8>>> {
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

/// Test view of an emission that cannot have split (no fast-emit offender):
/// its one piece.
#[cfg(test)]
pub(crate) fn single_piece(mut pieces: Vec<Vec<u8>>) -> Vec<u8> {
    assert_eq!(pieces.len(), 1, "expected one emitted piece");
    pieces.pop().expect("one piece")
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
/// This fail-closed backstop stops pathological IR before optimization.
/// Exceeding it never changes the rooting backend or optimization level.
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

/// One function whose rewritten IR exceeds the fail-closed compile budget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Rs4gcBudgetCause {
    /// RS4GC finished, but its relocation fan-out made the rewritten body too
    /// large for the normal optimization pipeline.
    PostRewrite { post_instructions: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rs4gcBudgetViolation {
    /// LLVM symbol whose optimization is refused.
    pub name: String,
    /// Instruction count before RS4GC, when the caller requested a census.
    pub pre_instructions: Option<usize>,
    /// Actual post-rewrite size that exceeded the budget.
    pub cause: Rs4gcBudgetCause,
    /// Active limit for the cause's estimate.
    pub cap: usize,
}

/// A compile error before optimization of an oversized rewritten function.
/// Rooting remains statepoints; callers never re-lower onto another backend.
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
/// tests spuriously fail.
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

/// Names of functions that actually entered RS4GC. The budget applies to
/// rewritten managed code, not unrelated ordinary functions in the module.
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
/// same conservative model as the statepoint rewrite — each
/// safepoint can leave one additional pointer result live across later calls —
/// but it observes the calls codegen actually emitted. That closes estimator
/// holes where one source expression expands into several collecting helpers.
fn rs4gc_call_may_collect(i: inkwell::values::InstructionValue<'_>) -> bool {
    if !matches!(
        i.get_opcode(),
        inkwell::values::InstructionOpcode::Call
            | inkwell::values::InstructionOpcode::CallBr
            | inkwell::values::InstructionOpcode::Invoke
    ) {
        return false;
    }
    let call = unsafe { inkwell::values::CallSiteValue::new(i.as_value_ref()) };
    let leaf = call
        .get_string_attribute(
            inkwell::attributes::AttributeLoc::Function,
            "gc-leaf-function",
        )
        .is_some();
    let callee_leaf = call.get_called_fn_value().is_some_and(|callee| {
        callee.get_intrinsic_id() != 0
            || callee
                .get_string_attribute(
                    inkwell::attributes::AttributeLoc::Function,
                    "gc-leaf-function",
                )
                .is_some()
    });
    !leaf && !callee_leaf
}

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
                    if rs4gc_call_may_collect(i) {
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

fn rewrite_budget_message(violation: &Rs4gcBudgetViolation, fatal: bool) -> String {
    let outcome = if fatal {
        "compilation stops while preserving mandatory statepoint rooting"
    } else {
        "the warning-only budget override leaves the function for LLVM to optimize"
    };
    match &violation.cause {
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
