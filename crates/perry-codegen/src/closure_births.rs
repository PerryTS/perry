//! Runs of fresh closure births become data.
//!
//! A run of closures that differ only in constants (body, which capture words,
//! where each closure goes) is emitted as a read-only table of birth records
//! plus ONE runtime call that walks it (`perry_runtime::closure::births`),
//! instead of one inline birth, store and barrier per closure. The runtime
//! bears exactly the closures the inline lowering bears, in the same order:
//! the same body info, the same birth shape, the same capture words. Run-once
//! code and hot code take the same path; only the code size changes.
//!
//! Two shapes carry such runs:
//! - a statement list's consecutive declarations whose initializer is a fresh
//!   closure and whose binding lives in a scope object of this frame (hoisted
//!   function declarations are the common case): the table names each
//!   closure's scope slot ([`try_lower_let_run`]);
//! - a constant literal tree whose leaves include fresh closures (an object
//!   literal of arrow getters is the common case): the literal descriptor
//!   gains a closure node (`expr::literal_descriptor`).
//!
//! The capture words are computed once per run, by the code an inline birth
//! uses (`expr::closure::push_capture_word`), and passed in a frame buffer.
//! Every word is a cell, a scope object or a binding's current value; a run
//! never contains a write to a binding it captures by value, because its
//! statements only bind scope-slot bindings, whose capture word is the scope
//! object itself.

use std::collections::HashMap;

use anyhow::Result;
use perry_hir::{Expr, Stmt};

use crate::expr::FnCtx;
use crate::types::{I32, I64, PTR};

/// Keep in sync with perry_runtime::closure::births::ClosureBirth.
pub(crate) const BIRTH_TYPE: &str = "{ ptr, i32, i32, i32, i32 }";

/// Fewer births than this stay inline: the table and the call cost about as
/// much code as the births they replace.
pub(crate) const MIN_RUN: usize = 8;

/// A fresh closure the table path bears exactly as `expr::closure::lower`
/// would: an ordinary fresh birth (no compiler-private singleton), no
/// receiver or `new.target` slot to patch afterwards, and no capture whose
/// representation the birth changes.
pub(crate) struct PlainBirth {
    func_id: u32,
    captures: Vec<u32>,
    is_async: bool,
}

pub(crate) fn plain_birth(ctx: &FnCtx<'_>, expr: &Expr) -> Option<PlainBirth> {
    let Expr::Closure {
        func_id,
        params,
        body,
        captures,
        captures_this,
        captures_new_target,
        is_async,
        ..
    } = expr
    else {
        return None;
    };
    if *captures_this || *captures_new_target {
        return None;
    }
    // Compiler-private async step closures take the singleton paths.
    if crate::expr::plain_async_step_release_ids(body).is_some() {
        return None;
    }
    let captures = crate::type_analysis::compute_auto_captures(ctx, params, body, captures);
    for &id in &captures {
        // A birth demotes a buffer alias or materializes `arguments` first.
        if ctx.receiver_descriptors.contains_buffer_view(id)
            || ctx.known_noalias_buffer_locals.contains(&id)
            || ctx.elided_arguments.contains_key(&id)
        {
            return None;
        }
        // A cell binding without storage here would get a fresh cell minted
        // at its birth; such a closure stays inline.
        if crate::stmt::binding_cell::capture_has_no_storage(ctx, id) {
            return None;
        }
    }
    Some(PlainBirth {
        func_id: *func_id,
        captures,
        is_async: *is_async,
    })
}

/// Where a source word comes from. Bindings that share storage (the members
/// of one scope object, read through the same frame root or capture slot)
/// share one source word.
#[derive(Clone, PartialEq, Eq, Hash)]
enum SourceKey {
    Root(String),
    Capture(u32),
    Binding(u32),
}

fn source_key(ctx: &FnCtx<'_>, id: u32) -> SourceKey {
    if ctx.boxed_vars.contains(&id) {
        if let Some(&idx) = ctx.closure_captures.get(&id) {
            return SourceKey::Capture(idx);
        }
        if let Some(slot) = ctx.locals.get(&id) {
            return SourceKey::Root(slot.clone());
        }
    }
    SourceKey::Binding(id)
}

/// One run's birth records and source words.
#[derive(Default)]
pub(crate) struct Births {
    records: Vec<String>,
    capture_indices: Vec<u32>,
    /// One binding per source word, in first-use order.
    sources: Vec<u32>,
    source_index: HashMap<SourceKey, u32>,
    /// Every capture of every birth, for the facts a birth records.
    captured: Vec<u32>,
    async_bodies: Vec<String>,
}

impl Births {
    fn source(&mut self, ctx: &FnCtx<'_>, id: u32) -> u32 {
        let key = source_key(ctx, id);
        if let Some(&index) = self.source_index.get(&key) {
            return index;
        }
        let index = self.sources.len() as u32;
        self.sources.push(id);
        self.source_index.insert(key, index);
        index
    }

    /// Add `birth`'s record, storing into scope slot `dest_slot` of the scope
    /// object read through `dest` (0, 0 for a descriptor node). Returns the
    /// record's index.
    pub(crate) fn add(
        &mut self,
        ctx: &mut FnCtx<'_>,
        birth: &PlainBirth,
        dest: Option<(u32, u32)>,
    ) -> u32 {
        let func_name =
            crate::fn_info::closure_body_symbol(ctx.strings.module_prefix(), birth.func_id);
        let info = ctx.block().fn_info_ref(&func_name);
        if birth.is_async {
            self.async_bodies.push(func_name);
        }
        let count = perry_abi::checked_closure_capture_count(birth.captures.len());
        let first = self.capture_indices.len() as u32;
        for &id in &birth.captures {
            let source = self.source(ctx, id);
            self.capture_indices.push(source);
            self.captured.push(id);
        }
        let (dest_source, dest_slot) = match dest {
            Some((id, slot)) => (self.source(ctx, id), slot),
            None => (0, 0),
        };
        let index = self.records.len() as u32;
        self.records.push(format!(
            "{BIRTH_TYPE} {{ ptr {info}, i32 {count}, i32 {first}, i32 {dest_source}, i32 {dest_slot} }}"
        ));
        index
    }

    /// Emit the record and capture-index tables and the source word buffer.
    /// Returns the call operands (records, count, indices, index count,
    /// words, word count) and the birth words to release after the call.
    pub(crate) fn emit(
        self,
        ctx: &mut FnCtx<'_>,
    ) -> Result<(
        Vec<(crate::types::LlvmType, String)>,
        crate::stmt::binding_cell::BirthWords,
    )> {
        // The facts each inline birth records (`expr::closure::lower`).
        for body in &self.async_bodies {
            ctx.block().note_fn_info(body, |facts| facts.set_async());
        }
        for &id in &self.captured {
            crate::expr::downgrade_buffer_alias(
                ctx,
                id,
                crate::native_value::MaterializationReason::ClosureCapture,
            );
        }
        let site = ctx.ic_site_counter;
        ctx.ic_site_counter += 1;
        let function: String = ctx
            .func
            .name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        let name = format!("perry_births_{function}_{site}");
        let count = self.records.len();
        ctx.typed_parse_rodata.push(format!(
            "@{name} = private unnamed_addr constant [{count} x {BIRTH_TYPE}] [{}]",
            self.records.join(", ")
        ));
        let index_count = self.capture_indices.len();
        let indices = if index_count == 0 {
            "null".to_string()
        } else {
            ctx.typed_parse_rodata.push(format!(
                "@{name}_captures = private unnamed_addr constant [{index_count} x i32] [{}]",
                self.capture_indices
                    .iter()
                    .map(|i| format!("i32 {i}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            format!("@{name}_captures")
        };
        // The words are read once, by the code an inline birth runs per
        // closure: cells first (`ensure_capture_cells`), then the words.
        crate::stmt::binding_cell::ensure_capture_cells(ctx, &self.sources);
        let mut birth = crate::stmt::binding_cell::BirthWords::new(ctx, &self.sources);
        for &id in &self.sources {
            crate::expr::push_capture_word(ctx, id, &mut birth)?;
        }
        let words = birth.read(ctx);
        let word_count = words.len();
        let buffer = if word_count == 0 {
            "null".to_string()
        } else {
            let buffer = ctx.func.alloca_entry_array(I64, word_count);
            let blk = ctx.block();
            for (i, word) in words.iter().enumerate() {
                let slot = blk.gep(I64, &buffer, &[(I64, &i.to_string())]);
                blk.store(I64, word, &slot);
            }
            buffer
        };
        let operands = vec![
            (PTR, format!("@{name}")),
            (I32, count.to_string()),
            (PTR, indices),
            (I32, index_count.to_string()),
            (PTR, buffer),
            (I32, word_count.to_string()),
        ];
        Ok((operands, birth))
    }
}

/// The scope slot a declaration's closure goes to: a binding of this frame
/// that lives in a scope object read through a frame root, seeded without the
/// dead-zone sentinel, and declared through the preallocated-cell path
/// (`stmt::let_stmt::lower_let`'s reuse arm).
fn scope_dest(ctx: &FnCtx<'_>, id: u32) -> Option<(u32, u32)> {
    let slot = crate::scope_env::access::slot(ctx, id)?;
    if slot.tdz
        || !ctx.boxed_vars.contains(&id)
        || ctx.closure_captures.contains_key(&id)
        || ctx.trusted_box_capture_ptrs.contains_key(&id)
        || !ctx.prealloc_boxes.contains(&id)
        || !ctx.locals.contains_key(&id)
        || ctx.array_stack_records.contains_key(&id)
    {
        return None;
    }
    Some((id, slot.index))
}

fn let_birth(ctx: &FnCtx<'_>, stmt: &Stmt) -> Option<(u32, PlainBirth, (u32, u32))> {
    let Stmt::Let {
        id,
        init: Some(init),
        ..
    } = stmt
    else {
        return None;
    };
    let dest = scope_dest(ctx, *id)?;
    let birth = plain_birth(ctx, init)?;
    Some((*id, birth, dest))
}

/// Bear the closures of the run of scope-slot closure declarations at the
/// head of `stmts` from one table. Returns the run's length (0 when the head
/// is no run). The caller then lowers each declaration of the run with
/// `ctx.born_let` naming it: everything a declaration records happens as
/// before, only its birth and store are already done.
pub(crate) fn try_lower_let_run(ctx: &mut FnCtx<'_>, stmts: &[Stmt]) -> Result<usize> {
    let mut run = Vec::new();
    for stmt in stmts {
        match let_birth(ctx, stmt) {
            Some(entry) => run.push(entry),
            None => break,
        }
    }
    if run.len() < MIN_RUN {
        return Ok(0);
    }
    let mut births = Births::default();
    for (_, birth, dest) in &run {
        births.add(ctx, birth, Some(*dest));
    }
    let (operands, words) = births.emit(ctx)?;
    let args: Vec<(crate::types::LlvmType, &str)> =
        operands.iter().map(|(t, v)| (*t, v.as_str())).collect();
    ctx.block().call_void("js_closure_births_to_scope", &args);
    words.release(ctx);
    Ok(run.len())
}

#[cfg(test)]
#[path = "closure_births_tests.rs"]
pub(crate) mod tests;
