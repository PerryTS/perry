//! Owns the per-build floating-point flags every f64 emitter derives its FMF prefix from. Explicit instance state, because two modules in one build may compile with different FP options.
use super::*;

/// Per-build floating-point flags captured by each function/block builder.
///
/// These are intentionally explicit instance state. Older code used process-
/// global atomics set at `compile_module` entry, which was only sound while
/// every parallel module in a build shared identical FP options. Tests and
/// embedding callers can compile modules with different options concurrently,
/// so f64 emitters must derive their FMF prefix from the owning block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FpFlags {
    fast_math: bool,
    fp_contract_mode: FpContractMode,
}

impl FpFlags {
    pub fn new(fast_math: bool, fp_contract_mode: FpContractMode) -> Self {
        Self {
            fast_math,
            fp_contract_mode,
        }
    }

    /// Inserted between the opcode and type in fp instructions:
    /// `fadd reassoc contract double …` vs `fadd double …`.
    pub(super) fn fmf_prefix(self) -> &'static str {
        match (self.fast_math, self.fp_contract_mode.permits_contract()) {
            (false, false) => "",
            (false, true) => "contract ",
            (true, false) => "reassoc ",
            (true, true) => "reassoc contract ",
        }
    }
}

impl Default for FpFlags {
    fn default() -> Self {
        Self::new(false, FpContractMode::Off)
    }
}
