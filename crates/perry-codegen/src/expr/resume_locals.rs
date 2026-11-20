//! Resume-local copies of the private generator controls (RLF).
//!
//! One resume of a private sync frame is exclusive: the transform's
//! `executing` control rejects re-entry before any other control is read, and
//! `generator_control_slots.rs` establishes that the state, completion kind
//! and done controls are inaccessible to JS and pointer-free. Within the
//! resume they are therefore ordinary SSA values: loaded once from the frame
//! at entry into entry allocas (promoted by mem2reg, so `state = K` feeds the
//! next dispatch directly) and written back to the frame on every exit.
//!
//! Exits are the function's `ret`s and its explicit throws, which are the
//! only places an activation leaves the resume with the controls meaningful:
//! an unwind that escapes passes through the transform's protocol catch,
//! which completes the generator first. The `executing` control, the sent
//! value, and every slot a callee or the consumer can observe are never
//! localized: they stay in the frame.
use super::*;
use crate::types::{I1, I32, I64};
use std::collections::BTreeMap;

/// Capture index of the `executing` control in the transform's fixed order
/// `[state, done, sent, executing, step, proto]`.
const EXECUTING_INDEX: u32 = 3;
/// Capture index of the completion kind (`step`).
pub(crate) const KIND_INDEX: u32 = 4;

pub(crate) struct Slot {
    ptr: String,
    boolean: bool,
    /// Frame word, or none for the operation parameter, which is a plain
    /// argument of the private ABI and has no frame slot.
    index: Option<u32>,
    written: bool,
    /// The parameter local this control shadows. It is kept equal so a generic
    /// read of the local stays correct; with no such read it is a dead store.
    mirror: Option<String>,
}

/// The state dispatch switch being lowered: where a known state value goes.
struct Dispatch {
    state: u32,
    targets: BTreeMap<i32, String>,
    /// Index into loop_targets of the loop the dispatch switch sits in.
    loop_index: usize,
    try_depth: usize,
}

pub(crate) struct ResumeLocals {
    slots: BTreeMap<u32, Slot>,
    dispatch: Option<Dispatch>,
    /// The sent parameter of a private sync entry (always undefined there).
    sent_param: Option<u32>,
    /// The state control holds this constant in this block, straight-line.
    known_state: Option<(u32, i32, usize)>,
    /// Runtime-handle stack height read once at entry, when the body has a try.
    /// A resume is its own activation and a generated frame holds no
    /// open runtime-handle scope between calls (scopes are RAII-balanced
    /// around each call), so the height at every try entry in this body
    /// equals the height at entry.
    eh_height: Option<String>,
}

fn contains_try(stmts: &[perry_hir::Stmt]) -> bool {
    use perry_hir::Stmt;
    stmts.iter().any(|s| match s {
        Stmt::Try { .. } => true,
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => contains_try(then_branch) || else_branch.as_deref().is_some_and(contains_try),
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
            contains_try(body)
        }
        Stmt::Labeled { body, .. } => contains_try(std::slice::from_ref(body.as_ref())),
        Stmt::Switch { cases, .. } => cases.iter().any(|c| contains_try(&c.body)),
        _ => false,
    })
}

/// The entry height for a try scope, when this resume cached one.
pub(crate) fn cached_eh_height(ctx: &FnCtx<'_>) -> Option<String> {
    if !ctx.inline_ctor_return.is_empty() || ctx.shared_super_scope_active {
        return None;
    }
    ctx.resume_locals.as_ref()?.eh_height.clone()
}

pub(super) fn managed(ctx: &FnCtx<'_>, id: u32) -> bool {
    ctx.resume_locals
        .as_ref()
        .is_some_and(|r| r.slots.contains_key(&id))
}

pub(super) fn ptr(ctx: &FnCtx<'_>, id: u32) -> Option<String> {
    ctx.resume_locals
        .as_ref()
        .and_then(|r| r.slots.get(&id))
        .map(|s| s.ptr.clone())
}

/// The localized completion kind, as an i32 value, when it is resume-local.
pub(crate) fn load_kind(ctx: &mut FnCtx<'_>) -> Option<String> {
    let ptr = ctx
        .resume_locals
        .as_ref()?
        .slots
        .values()
        .find(|s| s.index == Some(KIND_INDEX) && !s.boolean)?
        .ptr
        .clone();
    Some(ctx.block().load(I32, &ptr))
}

fn frame_ptr(ctx: &mut FnCtx<'_>, owner: &str, index: u32) -> String {
    let index = super::private_resume_frame::slot_index(ctx, index);
    let offset = super::private_resume_frame::slot_offset(ctx, &index);
    let address = ctx.block().add(I64, owner, &offset.to_string());
    ctx.block().inttoptr(I64, &address)
}

/// Localize the controls of a private sync resume body. Called once at entry.
pub(crate) fn enter(
    ctx: &mut FnCtx<'_>,
    params: &[perry_hir::Param],
    body: &[perry_hir::Stmt],
) -> Result<()> {
    if !(ctx.private_resume_frame && !ctx.private_async_resume_frame && ctx.fixed_generator_state) {
        return Ok(());
    }
    let mut writes = BTreeMap::new();
    crate::stmt::stable_packed_accumulator::collect_local_writes(body, &mut writes);
    let mut candidates: Vec<(u32, u32, bool)> = Vec::new();
    for (&id, &index) in ctx.closure_captures.iter() {
        if index == EXECUTING_INDEX || ctx.boxed_vars.contains(&id) {
            continue;
        }
        let i32_control = ctx.compiler_private_async_i32_control_locals.contains(&id);
        let i1_control = ctx.compiler_private_async_i1_control_locals.contains(&id);
        if i32_control || i1_control {
            candidates.push((id, index, i1_control));
        }
    }
    candidates.sort();
    let owner = current_closure_ptr_value(ctx, "resume locals")?;
    let mut slots = BTreeMap::new();
    for (id, index, boolean) in candidates {
        let local = ctx.func.alloca_entry(I32);
        let frame = frame_ptr(ctx, &owner, index);
        let value = if boolean {
            let bits = ctx.block().load(I64, &frame);
            let bit = ctx.block().icmp_eq(I64, &bits, crate::nanbox::TAG_TRUE_I64);
            ctx.block().zext(I1, &bit, I32)
        } else {
            let raw = ctx.block().load(DOUBLE, &frame);
            ctx.block().fptosi(DOUBLE, &raw, I32)
        };
        ctx.block().store(I32, &value, &local);
        slots.insert(
            id,
            Slot {
                ptr: local,
                boolean,
                index: Some(index),
                written: writes.contains_key(&id),
                mirror: None,
            },
        );
    }
    // The private operation is an i32 argument. Carry it as one: abrupt
    // routing writes 0 back, and a double slot would force a phi of doubles
    // that no later integer compare can fold.
    if let Some(op) = params.first().map(|p| p.id) {
        if ctx.compiler_private_async_i32_control_locals.contains(&op) {
            if let Some(mirror) = ctx.locals.get(&op).cloned() {
                let local = ctx.func.alloca_entry(I32);
                ctx.block().store(I32, "%frame_op", &local);
                slots.insert(
                    op,
                    Slot {
                        ptr: local,
                        boolean: false,
                        index: None,
                        written: false,
                        mirror: Some(mirror),
                    },
                );
            }
        }
    }
    let eh_height = contains_try(body).then(|| ctx.block().call(I64, "js_eh_handle_height", &[]));
    ctx.resume_locals = Some(ResumeLocals {
        slots,
        dispatch: None,
        sent_param: (params.len() == 2).then(|| params[1].id),
        known_state: None,
        eh_height,
    });
    Ok(())
}

/// Write every localized control that the body can change back to its frame
/// slot. Emitted before every exit of the resume.
pub(crate) fn write_back(ctx: &mut FnCtx<'_>) -> Result<()> {
    let Some(state) = ctx.resume_locals.as_ref() else {
        return Ok(());
    };
    let exits: Vec<(String, bool, u32)> = state
        .slots
        .values()
        .filter(|s| s.written)
        .filter_map(|s| s.index.map(|index| (s.ptr.clone(), s.boolean, index)))
        .collect();
    if exits.is_empty() {
        return Ok(());
    }
    ctx.block().emit_raw("  ; rlf.write_back");
    let owner = current_closure_ptr_value(ctx, "resume local write-back")?;
    for (local, boolean, index) in exits {
        let value = ctx.block().load(I32, &local);
        let frame = frame_ptr(ctx, &owner, index);
        if boolean {
            let bit = ctx.block().icmp_ne(I32, &value, "0");
            let bits = ctx.block().select(
                I1,
                &bit,
                I64,
                crate::nanbox::TAG_TRUE_I64,
                crate::nanbox::TAG_FALSE_I64,
            );
            // GC_STORE_AUDIT(POINTER_FREE): compiler-owned boolean control.
            ctx.block().store(I64, &bits, &frame);
        } else {
            let boxed = ctx.block().sitofp(I32, &value, DOUBLE);
            // GC_STORE_AUDIT(POINTER_FREE): compiler-owned i32 control.
            ctx.block().store(DOUBLE, &boxed, &frame);
        }
    }
    Ok(())
}

/// Generic (boxed-value) read of a localized control.
pub(crate) fn read_value(ctx: &mut FnCtx<'_>, id: u32) -> Option<String> {
    let (local, boolean) = {
        let slot = ctx.resume_locals.as_ref()?.slots.get(&id)?;
        (slot.ptr.clone(), slot.boolean)
    };
    let value = ctx.block().load(I32, &local);
    Some(if boolean {
        let bit = ctx.block().icmp_ne(I32, &value, "0");
        let bits = ctx.block().select(
            I1,
            &bit,
            I64,
            crate::nanbox::TAG_TRUE_I64,
            crate::nanbox::TAG_FALSE_I64,
        );
        ctx.block().bitcast_i64_to_double(&bits)
    } else {
        ctx.block().sitofp(I32, &value, DOUBLE)
    })
}

/// Generic (boxed-value) write of a localized control.
pub(crate) fn write_value(ctx: &mut FnCtx<'_>, id: u32, value: &str) -> bool {
    let Some((local, boolean)) = ctx
        .resume_locals
        .as_ref()
        .and_then(|r| r.slots.get(&id))
        .map(|s| (s.ptr.clone(), s.boolean))
    else {
        return false;
    };
    let narrowed = if boolean {
        let bits = ctx.block().bitcast_double_to_i64(value);
        let bit = ctx.block().icmp_eq(I64, &bits, crate::nanbox::TAG_TRUE_I64);
        ctx.block().zext(I1, &bit, I32)
    } else {
        ctx.block().fptosi(DOUBLE, value, I32)
    };
    ctx.block().store(I32, &narrowed, &local);
    if let Some(r) = ctx.resume_locals.as_mut() {
        r.known_state = None;
    }
    true
}

/// Keep the shadowed parameter local equal to a stored operation value.
pub(crate) fn mirror(ctx: &mut FnCtx<'_>, id: u32, value: &str) {
    let Some(local) = ctx
        .resume_locals
        .as_ref()
        .and_then(|r| r.slots.get(&id))
        .and_then(|s| s.mirror.clone())
    else {
        return;
    };
    let wide = ctx.block().sitofp(I32, value, DOUBLE);
    ctx.block().store(DOUBLE, &wide, &local);
}

/// A store of the state control: remember a constant for the straight-line
/// continuation of this block, forget anything else.
pub(crate) fn note_store(ctx: &mut FnCtx<'_>, id: u32, value: &str) {
    let block = ctx.current_block;
    if let Some(state) = ctx.resume_locals.as_mut() {
        state.known_state = value.parse::<i32>().ok().map(|k| (id, k, block));
    }
}

/// The dispatch switch of a fixed-state generator is about to lower its
/// case bodies. The targets map each case value to its body label.
pub(crate) fn begin_dispatch(
    ctx: &mut FnCtx<'_>,
    state: u32,
    targets: BTreeMap<i32, String>,
    loop_index: usize,
) {
    let try_depth = ctx.try_depth;
    if let Some(r) = ctx.resume_locals.as_mut() {
        if managed_slot(r, state) {
            r.dispatch = Some(Dispatch {
                state,
                targets,
                loop_index,
                try_depth,
            });
        }
    }
}

fn managed_slot(r: &ResumeLocals, id: u32) -> bool {
    r.slots.get(&id).is_some_and(|s| s.index.is_some())
}

pub(crate) fn end_dispatch(ctx: &mut FnCtx<'_>) {
    if let Some(r) = ctx.resume_locals.as_mut() {
        r.dispatch = None;
    }
}

/// A continue of the dispatch loop right after state = K re-enters the
/// switch with a known value: branch straight to that case body, which is
/// exactly where the switch would send it. Same loop, same try nesting,
/// same block, so no scope is crossed and no instruction intervenes.
pub(crate) fn direct_dispatch_target(ctx: &FnCtx<'_>, loop_index: usize) -> Option<String> {
    let r = ctx.resume_locals.as_ref()?;
    let d = r.dispatch.as_ref()?;
    let (id, k, block) = r.known_state?;
    (id == d.state
        && block == ctx.current_block
        && loop_index == d.loop_index
        && ctx.try_depth == d.try_depth)
        .then(|| d.targets.get(&k).cloned())
        .flatten()
}

pub(crate) fn is_sent_param(ctx: &FnCtx<'_>, id: u32) -> bool {
    ctx.resume_locals
        .as_ref()
        .is_some_and(|r| r.sent_param == Some(id))
}
