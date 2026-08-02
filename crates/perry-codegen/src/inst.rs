//! Typed instruction storage for `LlBlock` (exp/llvm-inprocess, Phase 2b).
//!
//! `Raw` carries a pre-rendered line exactly as the string emitter produced
//! it (two-space indent included), so introducing this enum changes zero
//! bytes of rendered IR — the migration gate. Typed variants land
//! opcode-by-opcode as `block.rs`'s semantic methods move off `format!`;
//! each variant must (1) render byte-identically to what the method
//! previously formatted and (2) be constructible natively by `dialect`'s
//! builder without a text round-trip. `PERRY_LLVM_INPROCESS=diff`'s
//! object-byte verdict gates every step; the raw-vs-typed instruction
//! counts are the migration ratchet.

pub enum LlInst {
    /// Pre-rendered instruction line, two-space indent included.
    Raw(String),
}

impl LlInst {
    /// Append the rendered line (no trailing newline) to `out`.
    pub fn render_into(&self, out: &mut String) {
        match self {
            LlInst::Raw(s) => out.push_str(s),
        }
    }

    /// Rendered byte length, excluding the trailing newline. Keeps
    /// `estimated_ir_bytes` (codegen-unit balancing, #5391) render-free.
    pub fn text_len(&self) -> usize {
        match self {
            LlInst::Raw(s) => s.len(),
        }
    }

    /// The rendered line for text-scanning consumers
    /// (`contains_gc_unsafe_call`). Typed variants must answer such
    /// predicates structurally rather than growing a renderer here.
    pub fn scan_str(&self) -> &str {
        match self {
            LlInst::Raw(s) => s,
        }
    }
}
