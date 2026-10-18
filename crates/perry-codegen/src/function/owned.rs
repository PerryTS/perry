//! Owned transport of a completed lowering graph. No reference-counted
//! builder handles cross the worker boundary. Exhaustive destructuring keeps
//! newly added builder fields visible to this conversion.

use super::*;

pub(crate) struct OwnedFunction {
    name: String,
    return_type: LlvmType,
    params: Vec<(LlvmType, String)>,
    linkage: String,
    force_inline: bool,
    pre_statepoint_inline: bool,
    preinline_after_naming: Option<PreinlineAfterNaming>,
    no_inline: bool,
    nanbox_operands: bool,
    inline_hint: bool,
    hot_loop_callee: bool,
    alloc_hot: bool,
    personality: Option<&'static str>,
    wpfacts_sites: u32,
    blocks: Vec<crate::block::OwnedBlock>,
    block_counter: u32,
    reg_counter: crate::block::OwnedCounter,
    fp_flags: FpFlags,
    entry_allocas: Vec<String>,
    staging_values: Option<(String, usize)>,
    staging_values_held: bool,
    entry_post_init_setup: Vec<String>,
    entry_init_boundary: Option<usize>,
    shadow_frame_slot: Option<String>,
    shadow_state_slot: Option<String>,
    shadow_frame_requested: bool,
    shadow_frame_post_init_region: bool,
    shadow_frame_push: Option<ShadowFramePush>,
    shadow_frame_slot_count: u32,
    stack_map_requested: bool,
    stack_map_slot_count: u32,
    outline_straight_line_store_ics: bool,
    pre_return_void_calls: Vec<String>,
    entry_tls_addresses: Vec<(String, String, bool)>,
}

impl LlFunction {
    pub(crate) fn into_owned(self) -> OwnedFunction {
        let Self {
            name,
            return_type,
            params,
            linkage,
            force_inline,
            pre_statepoint_inline,
            preinline_after_naming,
            no_inline,
            nanbox_operands,
            inline_hint,
            hot_loop_callee,
            alloc_hot,
            personality,
            wpfacts_sites,
            blocks,
            block_counter,
            reg_counter,
            fp_flags,
            entry_allocas,
            staging_values,
            staging_values_held,
            entry_post_init_setup,
            entry_init_boundary,
            shadow_frame_slot,
            shadow_state_slot,
            shadow_frame_requested,
            shadow_frame_post_init_region,
            shadow_frame_push,
            shadow_frame_slot_count,
            stack_map_requested,
            stack_map_slot_count,
            outline_straight_line_store_ics,
            pre_return_void_calls,
            entry_tls_addresses,
        } = self;
        let blocks = blocks
            .into_iter()
            .map(LlBlock::into_owned)
            .collect::<Vec<_>>();
        let reg_counter = Rc::try_unwrap(reg_counter)
            .unwrap_or_else(|_| panic!("completed function retains an external register counter"))
            .into_owned();
        OwnedFunction {
            name,
            return_type,
            params,
            linkage,
            force_inline,
            pre_statepoint_inline,
            preinline_after_naming,
            no_inline,
            nanbox_operands,
            inline_hint,
            hot_loop_callee,
            alloc_hot,
            personality,
            wpfacts_sites,
            blocks,
            block_counter,
            reg_counter,
            fp_flags,
            entry_allocas,
            staging_values,
            staging_values_held,
            entry_post_init_setup,
            entry_init_boundary,
            shadow_frame_slot,
            shadow_state_slot,
            shadow_frame_requested,
            shadow_frame_post_init_region,
            shadow_frame_push,
            shadow_frame_slot_count,
            stack_map_requested,
            stack_map_slot_count,
            outline_straight_line_store_ics,
            pre_return_void_calls,
            entry_tls_addresses,
        }
    }
}

impl OwnedFunction {
    pub(crate) fn into_builder(self) -> LlFunction {
        let Self {
            name,
            return_type,
            params,
            linkage,
            force_inline,
            pre_statepoint_inline,
            preinline_after_naming,
            no_inline,
            nanbox_operands,
            inline_hint,
            hot_loop_callee,
            alloc_hot,
            personality,
            wpfacts_sites,
            blocks,
            block_counter,
            reg_counter,
            fp_flags,
            entry_allocas,
            staging_values,
            staging_values_held,
            entry_post_init_setup,
            entry_init_boundary,
            shadow_frame_slot,
            shadow_state_slot,
            shadow_frame_requested,
            shadow_frame_post_init_region,
            shadow_frame_push,
            shadow_frame_slot_count,
            stack_map_requested,
            stack_map_slot_count,
            outline_straight_line_store_ics,
            pre_return_void_calls,
            entry_tls_addresses,
        } = self;
        let reg_counter = Rc::new(reg_counter.into_builder());
        let blocks = blocks
            .into_iter()
            .map(|b| b.into_builder(Rc::clone(&reg_counter)))
            .collect();
        LlFunction {
            name,
            return_type,
            params,
            linkage,
            force_inline,
            pre_statepoint_inline,
            preinline_after_naming,
            no_inline,
            nanbox_operands,
            inline_hint,
            hot_loop_callee,
            alloc_hot,
            personality,
            wpfacts_sites,
            blocks,
            block_counter,
            reg_counter,
            fp_flags,
            entry_allocas,
            staging_values,
            staging_values_held,
            entry_post_init_setup,
            entry_init_boundary,
            shadow_frame_slot,
            shadow_state_slot,
            shadow_frame_requested,
            shadow_frame_post_init_region,
            shadow_frame_push,
            shadow_frame_slot_count,
            stack_map_requested,
            stack_map_slot_count,
            outline_straight_line_store_ics,
            pre_return_void_calls,
            entry_tls_addresses,
        }
    }
}

impl LlFunction {
    pub(crate) fn relocate_callable(&mut self, r: &crate::codegen::callable_reloc::Relocations) {
        for block in &mut self.blocks {
            for inst in block.insts_mut() {
                inst.relocate_callable(r);
            }
        }
        for text in self
            .entry_allocas
            .iter_mut()
            .chain(&mut self.entry_post_init_setup)
        {
            r.text(text);
        }
        for (name, _, _) in &mut self.entry_tls_addresses {
            r.symbol(name);
        }
        for name in &mut self.pre_return_void_calls {
            r.symbol(name);
        }
    }
}
