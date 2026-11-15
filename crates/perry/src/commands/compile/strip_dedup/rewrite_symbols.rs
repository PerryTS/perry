//! Rewrite a wrapper member's selected symbols without changing its archive identity.
//!
//! Small members use one objcopy invocation; larger lists split at a conservative
//! argument budget. ELF panic/unwind exclusions and original COFF symbol indices
//! apply before batching, so batch boundaries cannot change linkage names.

use std::path::Path;
use std::process::Command;

use super::object_format::object_is_elf;

// ELF panic/unwind definitions, including the compiler-emitted DW.ref symbol,
// must stay global so sibling members and PIE relocations can resolve them.
// Classification uses substrings to recognize both forms of the personality.
const RUST_PANIC_UNWIND_SYMBOL_PARTS: &[&str] = &[
    "__rust_drop_panic",
    "__rust_foreign_exception",
    "rust_begin_unwind",
    "rust_eh_personality",
    "__rust_abort",
    "rust_panic",
];

/// Panic/unwind personality shims (incl. the compiler-emitted
/// `DW.ref.rust_eh_personality`, which substring-matches `rust_eh_personality`).
/// These must not be `--localize-symbol`'d on ELF — it breaks PIE relocations.
pub(super) fn is_panic_unwind_symbol(symbol: &str) -> bool {
    RUST_PANIC_UNWIND_SYMBOL_PARTS
        .iter()
        .any(|part| symbol.contains(part))
}

pub(super) fn rewrite_member_symbols(
    objcopy: &Path,
    member_path: &Path,
    member: &str,
    symbols: &[String],
    is_coff: bool,
    archive_tag: &str,
    member_index: usize,
) -> std::io::Result<()> {
    // On ELF, localizing the panic/unwind personality symbols (including the
    // compiler-emitted `DW.ref.rust_eh_personality`) breaks PIE relocations
    // → "undefined hidden symbol ... can not be used when making a PIE
    // object". That dedup is only needed for tier-3 Mach-O (-Zbuild-std);
    // skip it for ELF members and keep localizing the allocator shims.
    let skip_panic_unwind = object_is_elf(member_path);
    // Leave room for platform quoting and UTF-16 expansion. A single argument
    // is indivisible; oversized symbols retain the previous single-symbol call.
    const ARGUMENT_BUDGET: usize = 8192;
    let base_bytes = argument_bytes(objcopy.as_os_str()) + argument_bytes(member_path.as_os_str());
    let mut command = Command::new(objcopy);
    let mut batch_bytes = base_bytes;
    let mut batch_symbols = Vec::new();
    for (symbol_index, symbol) in symbols.iter().enumerate() {
        if skip_panic_unwind && is_panic_unwind_symbol(symbol) {
            continue;
        }
        // COFF does not support localization. Original indices keep these names
        // unique across members even when filtering or batch boundaries change.
        let (flag, value) = if is_coff {
            let renamed =
                format!("__perry_wrapper_local_{archive_tag}_{member_index}_{symbol_index}");
            ("--redefine-sym", format!("{symbol}={renamed}"))
        } else {
            ("--localize-symbol", symbol.clone())
        };
        let bytes = argument_bytes(flag.as_ref()) + argument_bytes(value.as_ref());
        if !batch_symbols.is_empty() && batch_bytes + bytes > ARGUMENT_BUDGET {
            run_batch(&mut command, member_path, member, &batch_symbols)?;
            command = Command::new(objcopy);
            batch_symbols.clear();
            batch_bytes = base_bytes;
        }
        command.arg(flag).arg(value);
        batch_bytes += bytes;
        batch_symbols.push(symbol.as_str());
    }
    if !batch_symbols.is_empty() {
        run_batch(&mut command, member_path, member, &batch_symbols)?;
    }
    Ok(())
}

fn argument_bytes(argument: &std::ffi::OsStr) -> usize {
    argument.as_encoded_bytes().len() * 2 + 3
}

fn run_batch(
    command: &mut Command,
    member_path: &Path,
    member: &str,
    symbols: &[&str],
) -> std::io::Result<()> {
    let out = command.arg(member_path).output()?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(std::io::Error::other(format!(
            "failed to rewrite {} in {member}: {stderr}",
            symbols.join(", ")
        )));
    }
    Ok(())
}

#[cfg(test)]
#[path = "rewrite_symbols/tests.rs"]
mod tests;
