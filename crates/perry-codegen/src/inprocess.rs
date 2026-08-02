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
            let ptrs: Vec<*const std::os::raw::c_char> =
                argv.iter().map(|c| c.as_ptr()).collect();
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
    // Interpret the plan argv. Unknown dash-flags are an error on purpose:
    // silently ignoring a flag clang would have honored is how the two
    // backends drift apart without anyone noticing.
    let mut opt = '0';
    let mut mcpu_native = false;
    let mut explicit_cpu: Option<&str> = None;
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
            s if s.starts_with("-mcpu=") => explicit_cpu = Some(&s["-mcpu=".len()..]),
            s if s.starts_with("-march=") => explicit_cpu = Some(&s["-march=".len()..]),
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

    global_init(&mllvm);
    announce();

    let context = Context::create();
    let buf = MemoryBuffer::create_from_memory_range_copy(ll_text.as_bytes(), module_name);
    let module = context
        .create_module_from_ir(buf)
        .map_err(|e| anyhow!("LLVM IR parse error:\n{}", e.to_string()))?;
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
