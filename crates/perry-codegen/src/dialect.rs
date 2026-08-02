//! Reader for Perry's LLVM-IR dialect: consumes one finalized function's
//! text (`LlFunction::to_ir()` output) and constructs it natively in an
//! inkwell module. See `native_emit.rs` for where this sits in Phase 2.
//!
//! This is NOT a general LLVM parser — it accepts exactly the closed set of
//! instruction, type, and operand forms perry-codegen emits (bounded by the
//! corpus census in the experiment doc) and errors on anything else, so a
//! new emission form fails loudly at construction instead of silently
//! diverging.

use anyhow::{bail, Result};
use inkwell::context::Context;
use inkwell::module::Module;

/// Parse `fn_text` (a complete `define ... { ... }`) and build it into
/// `module`. Returns the number of instructions constructed.
pub(crate) fn add_function_from_text<'ctx>(
    _context: &'ctx Context,
    _module: &Module<'ctx>,
    fn_text: &str,
) -> Result<usize> {
    // Implementation lands with the dialect grammar work; the scaffold fails
    // loudly so `native`/`diff` modes can never silently no-op.
    let name = fn_text
        .lines()
        .next()
        .unwrap_or("")
        .split('@')
        .nth(1)
        .and_then(|s| s.split('(').next())
        .unwrap_or("?");
    bail!("dialect reader not yet implemented (function @{name})")
}
