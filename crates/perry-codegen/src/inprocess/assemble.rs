//! Assemble text with Perry's own LLVM (the MC layer that printed it).
//!
//! The statepoint backends print assembly so `gc_map` can rewrite the stack
//! map (#7314), and the rewritten text used to go to whatever `clang` the host
//! has. That clang can be older than the bundled LLVM, and an older assembler
//! rejects directives the bundled LLVM prints: on GitHub's windows-2022 image
//! (clang 20.1.8, Perry bundles LLVM 22) every Windows compile failed, first on
//! `.seh_startepilogue`, then on COFF `.section ...,unique,N`.
//!
//! The LLVM C API has no assembler entry point, but module-level inline asm
//! is parsed by the same `MCAsmParser` and streamed by the same object writer
//! `clang -c foo.s` uses. A module holding only the text, emitted as an
//! object, is therefore an assembler that cannot disagree with the LLVM that
//! printed the text. Diagnostics are collected through the context's handler:
//! without one LLVM prints an inline-asm error and calls `exit(1)`.

use super::{default_cpu_for_triple, global_init};
use anyhow::{anyhow, Result};
use inkwell::context::Context;
use inkwell::targets::{CodeModel, FileType, RelocMode, Target, TargetMachine, TargetTriple};
use inkwell::OptimizationLevel;
use std::ffi::{c_void, CStr};
use std::sync::Mutex;

extern "C" fn collect_diagnostic(
    info: llvm_sys::prelude::LLVMDiagnosticInfoRef,
    sink: *mut c_void,
) {
    // SAFETY: `sink` is the `Mutex` `assemble_text` installs for exactly the
    // lifetime of its context, and LLVM hands the info only for this call.
    unsafe {
        let severity = llvm_sys::core::LLVMGetDiagInfoSeverity(info);
        if severity != llvm_sys::LLVMDiagnosticSeverity::LLVMDSError {
            return;
        }
        let description = llvm_sys::core::LLVMGetDiagInfoDescription(info);
        let text = CStr::from_ptr(description).to_string_lossy().into_owned();
        llvm_sys::core::LLVMDisposeMessage(description);
        let sink = &*(sink as *const Mutex<Vec<String>>);
        if let Ok(mut errors) = sink.lock() {
            errors.push(text);
        }
    }
}

/// The CPU and features the assembler must be told: the same machine the
/// code generator was told (`gc_map::cpu_selection_flags`), or an aarch64
/// host's SVE instructions are rejected.
fn machine(target: &str, cpu_flags: &[&String]) -> (String, String) {
    let selected = cpu_flags.iter().rev().find_map(|flag| {
        flag.strip_prefix("-mcpu=")
            .or_else(|| flag.strip_prefix("-march="))
    });
    match selected {
        Some("native") => (
            TargetMachine::get_host_cpu_name()
                .to_string_lossy()
                .into_owned(),
            TargetMachine::get_host_cpu_features()
                .to_string_lossy()
                .into_owned(),
        ),
        Some(cpu) => (cpu.to_string(), String::new()),
        // `clang -c` on an x86_64 triple assembles for `x86-64`, not LLVM's
        // `generic`; the two pick different long-NOP padding.
        None if target.starts_with("x86_64") => ("x86-64".to_string(), String::new()),
        None => (default_cpu_for_triple(target).to_string(), String::new()),
    }
}

/// COFF: `text` without what the printer of the carrier module emits
/// itself: the `@feat.00` definition ("redefinition of '@feat.00'"
/// otherwise) and the single-parameter `.file "name"`, whose name is
/// returned so the carrier prints it instead (one `.file` symbol, as
/// `clang -c` gives).
///
/// The printer derives the value from module flags (`cfguard`,
/// `ehcontguard`, `ms-kernel`; SafeSEH on 32-bit x86), and the carrier has
/// none, so it prints 0 on x86_64. A text that says anything else would lose
/// a bit here: that is refused rather than dropped.
fn without_carrier_directives(text: &str) -> Result<(String, Option<String>)> {
    let mut out = String::with_capacity(text.len());
    let mut file = None;
    let mut lines = text.split_inclusive('\n').peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed == ".def\t@feat.00;" || trimmed == ".def @feat.00;" {
            // `.def @feat.00;` `.scl 3;` `.type 0;` `.endef`
            for rest in lines.by_ref() {
                if rest.trim() == ".endef" {
                    break;
                }
            }
            continue;
        }
        if trimmed == ".globl\t@feat.00" || trimmed == ".globl @feat.00" {
            continue;
        }
        if file.is_none() {
            if let Some(name) = trimmed
                .strip_prefix(".file\t\"")
                .or_else(|| trimmed.strip_prefix(".file \""))
                .and_then(|rest| rest.strip_suffix('"'))
            {
                if !name.contains('"') {
                    file = Some(name.to_string());
                    continue;
                }
            }
        }
        if let Some(value) = trimmed.strip_prefix("@feat.00 = ") {
            if value.trim() != "0" {
                return Err(anyhow!(
                    "the in-process assembler cannot carry `@feat.00 = {value}` \
                     (only 0 is supported)"
                ));
            }
            continue;
        }
        out.push_str(line);
    }
    Ok((out, file))
}

/// Object bytes for assembly `text` targeting `target`.
pub(crate) fn assemble_text(text: &str, target: &str, cpu_flags: &[&String]) -> Result<Vec<u8>> {
    global_init(&[]);
    let coff_text;
    let (text, file) = if target.contains("windows") && target.starts_with("x86_64") {
        let (stripped, file) = without_carrier_directives(text)?;
        coff_text = stripped;
        (coff_text.as_str(), file)
    } else {
        (text, None)
    };
    let triple = TargetTriple::create(target);
    let llvm_target =
        Target::from_triple(&triple).map_err(|e| anyhow!("no LLVM target for `{target}`: {e}"))?;
    let (cpu, features) = machine(target, cpu_flags);
    let tm = llvm_target
        .create_target_machine(
            &triple,
            &cpu,
            &features,
            OptimizationLevel::None,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or_else(|| anyhow!("failed to create TargetMachine for `{target}`"))?;
    let errors: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let context = Context::create();
    // SAFETY: the handler only reads `errors`, which outlives `context`
    // (declared first, dropped last).
    unsafe {
        llvm_sys::core::LLVMContextSetDiagnosticHandler(
            context.raw(),
            Some(collect_diagnostic),
            &errors as *const Mutex<Vec<String>> as *mut c_void,
        );
    }
    let module = context.create_module("perry_assembly");
    // The carrier prints `.file` with its source file name (COFF/ELF), so it
    // carries the text's own.
    let file = file.unwrap_or_default();
    unsafe {
        llvm_sys::core::LLVMSetSourceFileName(module.as_mut_ptr(), file.as_ptr().cast(), file.len())
    };
    module.set_triple(&triple);
    module.set_data_layout(&tm.get_target_data().get_data_layout());
    // SAFETY: the C API copies the bytes into the module.
    unsafe {
        llvm_sys::core::LLVMSetModuleInlineAsm2(
            module.as_mut_ptr(),
            text.as_ptr().cast(),
            text.len(),
        );
    }
    let emitted = tm.write_to_memory_buffer(&module, FileType::Object);
    drop(module);
    drop(context);
    let errors = errors.into_inner().unwrap_or_default();
    if !errors.is_empty() {
        return Err(anyhow!(
            "LLVM {} could not assemble the text it printed:\n{}",
            llvm_version(),
            errors.join("\n")
        ));
    }
    let buffer = emitted.map_err(|e| anyhow!("assembling for `{target}` failed: {e}"))?;
    Ok(buffer.as_slice().to_vec())
}

fn llvm_version() -> String {
    let (mut major, mut minor, mut patch) = (0u32, 0u32, 0u32);
    unsafe { llvm_sys::core::LLVMGetVersion(&mut major, &mut minor, &mut patch) };
    format!("{major}.{minor}.{patch}")
}

#[cfg(test)]
#[path = "assemble_tests.rs"]
mod tests;
