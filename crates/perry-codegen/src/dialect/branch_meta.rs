//! Branch attachments of the text reader: loop IDs and version marks.

use anyhow::Result;
use inkwell::values::InstructionValue;

use super::text::attach_trailing_metadata;
use super::FnReader;

impl<'ctx, 'm> FnReader<'ctx, 'm> {
    /// The attachments `BranchMeta::render_into` writes on the text path,
    /// built on the same nodes: the latch's numbered `!llvm.loop` ID and the
    /// byte-view admission's `!perry.byte_view.version !{}`.
    pub(super) fn branch_meta(
        &self,
        inst: InstructionValue<'ctx>,
        meta: Option<&crate::inst::BranchMeta>,
    ) -> Result<()> {
        let Some(meta) = meta else { return Ok(()) };
        let mut parts = Vec::new();
        if let Some(tag) = &meta.loop_tag {
            if tag.id != 0 {
                parts.push(format!("!llvm.loop !{}", tag.id));
            }
        }
        if meta.byte_view_version {
            parts.push("!perry.byte_view.version !{}".to_string());
        }
        attach_trailing_metadata(self.ctx, self.metadata, Some(inst), &parts)
    }
}
