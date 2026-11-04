//! Numbered metadata ids of a module: the one counter, the per-function
//! buffer alias scope allocator, and loop-ID numbering.

use std::collections::HashMap;

use super::{llvm_metadata_string, LlModule};

/// The buffer alias scopes one function allocates. A scope owns three
/// consecutive numbered metadata ids from the module counter: the scope node
/// `S`, its single-scope list `S+1` (`!alias.scope`) and its noalias list
/// `S+2` (every other scope). The scope id is the buffer's `scope_idx`.
#[derive(Clone, Debug, Default)]
pub struct BufferAliasScopes {
    pub(super) base: u32,
    pub(super) scopes: Vec<u32>,
}

impl BufferAliasScopes {
    /// A fresh scope id, never handed out before in this module.
    pub fn allocate(&mut self) -> u32 {
        let id = self.base + 3 * self.scopes.len() as u32;
        self.scopes.push(id);
        id
    }
}

impl LlModule {
    /// Take the next numbered metadata id from the module counter.
    pub(crate) fn allocate_metadata_id(&mut self) -> u32 {
        let id = self.next_metadata_id;
        self.next_metadata_id += 1;
        id
    }

    /// The allocator a function's `FnCtx` takes its buffer alias scopes
    /// from: ids continue the module counter. Functions are lowered one at a
    /// time; [`Self::commit_buffer_alias_scopes`] returns the ids before the
    /// next function starts.
    pub(crate) fn buffer_alias_scope_allocator(&self) -> BufferAliasScopes {
        BufferAliasScopes {
            base: self.next_metadata_id,
            scopes: Vec::new(),
        }
    }

    /// Publish the scopes one function allocated and move the module
    /// counter past their ids.
    pub(crate) fn commit_buffer_alias_scopes(&mut self, scopes: BufferAliasScopes) {
        if scopes.scopes.is_empty() {
            return;
        }
        assert_eq!(
            scopes.base, self.next_metadata_id,
            "buffer alias scopes allocated from a stale metadata id counter"
        );
        self.next_metadata_id = scopes.base + 3 * scopes.scopes.len() as u32;
        self.buffer_alias_scopes.extend(scopes.scopes);
    }

    /// Scope-node ids of every committed buffer alias scope.
    pub(crate) fn buffer_alias_scopes(&self) -> &[u32] {
        &self.buffer_alias_scopes
    }

    /// Give every loop latch tag (see [`crate::inst::BranchMeta`]) its own
    /// numbered loop ID, once all functions are lowered. Each latch gets a
    /// distinct `!K = distinct !{!K, ...}` (LLVM loop IDs are per loop); the
    /// operand nodes (`!{!"perry.region", !"<id>"}` per region,
    /// `!{!"perry.byte_view.fallback"}`) are shared. Ids come from the
    /// module's one metadata id counter, so they cannot collide with the
    /// alias-scope tables. Idempotent: a numbered tag keeps its id.
    pub(crate) fn number_loop_metadata(&mut self) {
        let mut next = self.next_metadata_id;
        let mut operand_nodes: HashMap<String, u32> = HashMap::new();
        let mut lines = Vec::new();
        let mut operand = |text: String, next: &mut u32, lines: &mut Vec<String>| -> u32 {
            *operand_nodes.entry(text.clone()).or_insert_with(|| {
                let id = *next;
                *next += 1;
                lines.push(format!("!{id} = {text}"));
                id
            })
        };
        for function in &mut self.functions {
            for block in function.blocks_mut() {
                for inst in block.insts_mut() {
                    let meta = match inst {
                        crate::inst::LlInst::Br { meta, .. }
                        | crate::inst::LlInst::CondBr { meta, .. } => meta,
                        _ => continue,
                    };
                    let Some(tag) = meta.as_mut().and_then(|meta| meta.loop_tag.as_mut()) else {
                        continue;
                    };
                    if tag.id != 0 {
                        continue;
                    }
                    let mut operands = String::new();
                    if let Some(region) = &tag.region {
                        let text = format!(
                            "!{{!\"perry.region\", !\"{}\"}}",
                            llvm_metadata_string(region)
                        );
                        let id = operand(text, &mut next, &mut lines);
                        operands.push_str(&format!(", !{id}"));
                    }
                    if tag.byte_view_fallback {
                        let text = "!{!\"perry.byte_view.fallback\"}".to_string();
                        let id = operand(text, &mut next, &mut lines);
                        operands.push_str(&format!(", !{id}"));
                    }
                    let id = next;
                    next += 1;
                    tag.id = id;
                    lines.push(format!("!{id} = distinct !{{!{id}{operands}}}"));
                }
            }
        }
        self.next_metadata_id = next;
        self.metadata_lines.extend(lines);
    }
}
