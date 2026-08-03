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
    let (opt, mcpu_native, explicit_cpu, mllvm, emit_asm) = interpret_plan_args(clang_style_args)?;
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
    )
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
    )
}

fn optimize_and_emit(
    module: &inkwell::module::Module<'_>,
    effective_target: &str,
    opt: char,
    mcpu_native: bool,
    explicit_cpu: Option<&str>,
    mllvm: &[String],
    emit_asm: bool,
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
    if crate::codegen::helpers::rs4gc_enabled() {
        module
            .run_passes(
                "function(mem2reg),rewrite-statepoints-for-gc",
                &tm,
                PassBuilderOptions::create(),
            )
            .map_err(|e| {
                anyhow!(
                    "in-process rewrite-statepoints-for-gc failed:\n{}",
                    e.to_string()
                )
            })?;
    }

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

    let kind = if emit_asm {
        FileType::Assembly
    } else {
        FileType::Object
    };
    let obj = tm
        .write_to_memory_buffer(&module, kind)
        .map_err(|e| anyhow!("{kind:?} emission failed:\n{}", e.to_string()))?;
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
