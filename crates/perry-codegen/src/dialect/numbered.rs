//! Numbered module metadata (`!N = [distinct] !{...}`) for native
//! construction.
//!
//! The text path hands LLVM's parser the module's numbered metadata lines
//! (`LlModule::push_metadata` / the per-unit referenced subset) and every
//! `!alias.scope !N`, `!noalias !N` and `!llvm.loop !N` reference resolves
//! against them. Native construction parses the same skeleton text, but the
//! parser keeps no handle on a numbered node, so the reader builds the nodes
//! itself from those same lines: one source for both paths. Built lazily, per
//! LLVM context, once per id, so every function of a module shares one node
//! per id exactly as the text parse does (a distinct scope duplicated per
//! function would silently weaken alias facts).
//!
//! Accepted forms are the ones perry emits: tuples of `!"string"` leaves,
//! nested `!{...}` tuples and `!N` references. A self-referencing node is
//! built the way LLVM's parser builds one (a temporary operand replaced by
//! the node itself, which makes the node distinct), and that is the only way
//! `distinct` is accepted. Anything else (an unknown id, a `distinct` node
//! without a self reference, a self reference in a uniqued node, a longer
//! cycle, a non-tuple body) is a loud error, never a dropped attachment.

use std::cell::RefCell;
use std::collections::HashMap;

use anyhow::{anyhow, bail, Context as _, Result};
use inkwell::context::{AsContextRef, Context};
use inkwell::values::{AsValueRef, MetadataValue};
use llvm_sys::prelude::LLVMMetadataRef;

#[derive(Default)]
pub(crate) struct NumberedMetadata<'ctx> {
    defs: HashMap<u32, String>,
    built: RefCell<HashMap<u32, MetadataValue<'ctx>>>,
}

enum Md {
    Str(String),
    Tuple(Vec<Md>),
    Ref(u32),
}

/// `!N = ...` -> `(N, body)`.
pub(crate) fn numbered_definition(line: &str) -> Option<(u32, &str)> {
    let rest = line.strip_prefix('!')?;
    let (id, body) = rest.split_once(" = ")?;
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((id.parse().ok()?, body.trim()))
}

impl<'ctx> NumberedMetadata<'ctx> {
    /// No numbered metadata: every `!N` reference is an error.
    pub(crate) fn none() -> Self {
        Self::default()
    }

    /// The numbered definitions of a module (or unit skeleton) text.
    pub(crate) fn from_ir(ir: &str) -> Self {
        let defs = ir
            .lines()
            .filter_map(numbered_definition)
            .map(|(id, body)| (id, body.to_string()))
            .collect();
        Self {
            defs,
            built: RefCell::new(HashMap::new()),
        }
    }

    /// The node `!id`, built on first use.
    pub(crate) fn node(&self, ctx: &'ctx Context, id: u32) -> Result<MetadataValue<'ctx>> {
        let md = self.resolve(ctx, id, &mut Vec::new())?;
        Ok(
            unsafe {
                MetadataValue::new(llvm_sys::core::LLVMMetadataAsValue(ctx.as_ctx_ref(), md))
            },
        )
    }

    fn resolve(
        &self,
        ctx: &'ctx Context,
        id: u32,
        stack: &mut Vec<u32>,
    ) -> Result<LLVMMetadataRef> {
        if let Some(v) = self.built.borrow().get(&id) {
            return Ok(unsafe { llvm_sys::core::LLVMValueAsMetadata(v.as_value_ref()) });
        }
        if stack.contains(&id) {
            bail!("metadata cycle through !{id} (only direct self references are supported)");
        }
        let body = self
            .defs
            .get(&id)
            .ok_or_else(|| anyhow!("metadata !{id} is referenced but not defined"))?;
        let (distinct, tuple) = match body.strip_prefix("distinct ") {
            Some(rest) => (true, rest.trim()),
            None => (false, body.as_str()),
        };
        let (tree, rest) = parse(tuple).with_context(|| format!("metadata !{id} = {body}"))?;
        if !rest.trim().is_empty() {
            bail!("trailing text after metadata !{id} = {body}");
        }
        let self_ref = refers_to(&tree, id);
        match (distinct, self_ref) {
            (true, true) | (false, false) => {}
            (true, false) => bail!("distinct metadata !{id} without a self reference: {body}"),
            (false, true) => bail!("uniqued metadata !{id} refers to itself: {body}"),
        }
        let c = ctx.as_ctx_ref();
        let temp = self_ref.then(|| unsafe {
            llvm_sys::debuginfo::LLVMTemporaryMDNode(c, std::ptr::null_mut(), 0)
        });
        stack.push(id);
        let built = self.build(ctx, &tree, id, temp, stack);
        stack.pop();
        let node = built?;
        if let Some(temp) = temp {
            // LLVM drops uniquing of a node that becomes self-referencing:
            // the node is distinct, as `distinct !{!N, ...}` parses.
            unsafe { llvm_sys::debuginfo::LLVMMetadataReplaceAllUsesWith(temp, node) };
        }
        let value = unsafe { MetadataValue::new(llvm_sys::core::LLVMMetadataAsValue(c, node)) };
        self.built.borrow_mut().insert(id, value);
        Ok(node)
    }

    fn build(
        &self,
        ctx: &'ctx Context,
        md: &Md,
        id: u32,
        temp: Option<LLVMMetadataRef>,
        stack: &mut Vec<u32>,
    ) -> Result<LLVMMetadataRef> {
        let c = ctx.as_ctx_ref();
        Ok(match md {
            Md::Str(s) => unsafe {
                llvm_sys::core::LLVMMDStringInContext2(c, s.as_ptr() as *const _, s.len())
            },
            Md::Ref(r) if *r == id => temp.expect("self reference checked above"),
            Md::Ref(r) => self.resolve(ctx, *r, stack)?,
            Md::Tuple(elems) => {
                let mut ops = Vec::with_capacity(elems.len());
                for e in elems {
                    ops.push(self.build(ctx, e, id, temp, stack)?);
                }
                unsafe { llvm_sys::core::LLVMMDNodeInContext2(c, ops.as_mut_ptr(), ops.len()) }
            }
        })
    }
}

fn refers_to(md: &Md, id: u32) -> bool {
    match md {
        Md::Ref(r) => *r == id,
        Md::Tuple(elems) => elems.iter().any(|e| refers_to(e, id)),
        Md::Str(_) => false,
    }
}

fn parse(s: &str) -> Result<(Md, &str)> {
    let s = s.trim_start();
    if let Some(body) = s.strip_prefix("!\"") {
        let end = body
            .find('"')
            .ok_or_else(|| anyhow!("unterminated metadata string"))?;
        let text = super::text::unescape_llvm_bytes(&body[..end])?;
        return Ok((Md::Str(text), &body[end + 1..]));
    }
    if let Some(mut rest) = s.strip_prefix("!{") {
        let mut elems = Vec::new();
        loop {
            rest = rest.trim_start();
            if let Some(after) = rest.strip_prefix('}') {
                return Ok((Md::Tuple(elems), after));
            }
            let (elem, after) = parse(rest)?;
            elems.push(elem);
            rest = after.trim_start();
            if let Some(after) = rest.strip_prefix(',') {
                rest = after;
            } else if !rest.starts_with('}') {
                bail!("expected `,` or `}}` in metadata tuple at `{rest}`");
            }
        }
    }
    if let Some(rest) = s.strip_prefix('!') {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 {
            return Ok((Md::Ref(rest[..digits].parse()?), &rest[digits..]));
        }
    }
    bail!("unsupported metadata form `{s}`")
}
