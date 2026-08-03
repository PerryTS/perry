//! In-process `.ll -> .o` compilation through the LLVM C API (exp/llvm-inprocess).
//!
//! Feature-gated (`llvm-inprocess`) and flag-gated (`PERRY_LLVM_INPROCESS=1`):
//! the default build does not link LLVM, and a build that has the feature
//! still uses the `clang -c` subprocess unless the flag is set. Selection and
//! the flag's cache-key participation live in `linker.rs` /
//! `perry/src/commands/compile/{build_cache,object_cache}.rs`.
//!
//! Decision parity by construction: this module does not re-derive opt levels
//! or CPU tuning. It interprets the *same* argv `build_clang_compile_plan`
//! produces for clang (`-O3`/`-Os`/`-O0`, `-mcpu=native`, `-mllvm
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
        Target::initialize_all(&InitializationConfig::default());
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
) -> Result<Vec<u8>> {
    let (opt, mcpu_native, explicit_cpu, mllvm) = interpret_plan_args(clang_style_args)?;
    let context = Context::create();
    let module = parse_ir_text(&context, ll_text, module_name)?;
    optimize_and_emit(
        &module,
        effective_target,
        opt,
        mcpu_native,
        explicit_cpu.as_deref(),
        &mllvm,
    )
}

/// Interpret the plan argv. Unknown dash-flags are an error on purpose:
/// silently ignoring a flag clang would have honored is how the two
/// backends drift apart without anyone noticing.
#[allow(clippy::type_complexity)]
fn interpret_plan_args(
    clang_style_args: &[String],
) -> Result<(char, bool, Option<String>, Vec<String>)> {
    let mut opt = '0';
    let mut mcpu_native = false;
    let mut explicit_cpu: Option<String> = None;
    let mut mllvm: Vec<String> = Vec::new();
    let mut it = clang_style_args.iter().peekable();
    while let Some(a) = it.next() {
        match a.as_str() {
            // `-g` is a measured no-op on Perry IR (no DI metadata; see the
            // TEMP_NONCE_COUNTER doc block in linker.rs), matching clang.
            "-c" | "-fno-math-errno" | "-g" => {}
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
    Ok((opt, mcpu_native, explicit_cpu, mllvm))
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
) -> Result<Vec<u8>> {
    let (opt, mcpu_native, explicit_cpu, mllvm) = interpret_plan_args(clang_style_args)?;
    optimize_and_emit(
        module,
        effective_target,
        opt,
        mcpu_native,
        explicit_cpu.as_deref(),
        &mllvm,
    )
}

fn optimize_and_emit(
    module: &inkwell::module::Module<'_>,
    effective_target: &str,
    opt: char,
    mcpu_native: bool,
    explicit_cpu: Option<&str>,
    mllvm: &[String],
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
        (String::new(), String::new())
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

    let pipeline = match opt {
        '0' => "default<O0>",
        '1' => "default<O1>",
        '2' => "default<O2>",
        's' => "default<Os>",
        'z' => "default<Oz>",
        _ => "default<O3>",
    };
    module
        .run_passes(pipeline, &tm, PassBuilderOptions::create())
        .map_err(|e| anyhow!("pass pipeline `{pipeline}` failed:\n{}", e.to_string()))?;

    let obj = tm
        .write_to_memory_buffer(&module, FileType::Object)
        .map_err(|e| anyhow!("object emission failed:\n{}", e.to_string()))?;
    Ok(obj.as_slice().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
