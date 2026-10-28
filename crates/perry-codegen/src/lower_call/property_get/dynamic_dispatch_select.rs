//! Class-method selection by prototype constants (object model step 3,
//! `OBJECT_MODEL_2026-10-13.md` §3.6/§3.7 case 2).
//!
//! Child module of `dynamic_dispatch.rs`. A compiled case is a class whose
//! method resolves, through a prototype chain the compiler names
//! (`static_prototype::chain`), to a slot the program's write census proves
//! constant (`proto_constants::method_is_constant`). For a receiver of such a
//! class the method IS that body: no object can gain an own property of the
//! name, and no prototype slot of the name is ever rewritten or relinked. The
//! selection is therefore a branch over the receiver's class id against the
//! compile-time set of cased classes, with nothing loaded but the receiver's
//! header word, and no runtime proof, epoch or record. When the program has a
//! dynamic write or a relink, the receiver's layout word and each cased
//! class's chain holders must also carry no shadow bit
//! (`proto_constants::emit_receiver_unshadowed` / `holder_constant`). Every
//! other receiver takes the site's generic arm.

use super::*;
use crate::types::I32;

/// Is implementor `class`'s `property` a prototype constant?
pub(super) fn cased(ctx: &FnCtx<'_>, class: &str, property: &str, function: &str) -> bool {
    !function.starts_with("perry_static_")
        && !class_chain_has_field_named(ctx, class, property)
        && crate::proto_constants::class_method_is_constant(ctx, class, property)
}

/// Emit the selection; returns an `i32`: `i + 1` to call implementor `i`
/// directly (only a `cased` one), 0 for the generic arm.
pub(super) fn select(
    ctx: &mut FnCtx<'_>,
    recv: &str,
    property: &str,
    implementors: &[(u32, String)],
    cased: &[bool],
) -> String {
    let (word, admitted) =
        crate::lower_call::method_override::emit_direct_method_selection_probe(ctx, recv);
    select_with_word(ctx, recv, property, implementors, cased, &word, &admitted)
}

/// Select from an already loaded receiver header word (`class_id | ShapeId
/// << 32`) and its admission predicate. An unadmitted word cannot select a
/// body. `recv` is read only by contract builds.
pub(super) fn select_with_word(
    ctx: &mut FnCtx<'_>,
    recv: &str,
    property: &str,
    implementors: &[(u32, String)],
    cased: &[bool],
    word: &str,
    admitted: &str,
) -> String {
    let cid = ctx.block().trunc(I64, word, I32);
    let receiver_ok = crate::proto_constants::emit_receiver_unshadowed(ctx, word);
    let admitted = ctx.block().and(crate::types::I1, admitted, &receiver_ok);
    let done = ctx.new_block("pconst.selected");
    let done_l = ctx.block_label(done);
    let start_l = ctx.block().label.clone();
    let dispatch = ctx.new_block("pconst.admitted");
    let dispatch_l = ctx.block_label(dispatch);
    ctx.block().cond_br(&admitted, &dispatch_l, &done_l);
    ctx.current_block = dispatch;
    let mut seen = std::collections::HashSet::new();
    let cases: Vec<(usize, usize)> = implementors
        .iter()
        .enumerate()
        .filter(|&(i, (id, _))| cased[i] && seen.insert(*id))
        .map(|(i, _)| (i, ctx.new_block("pconst.case")))
        .collect();
    let mut switch = format!("switch i32 {cid}, label %{done_l} [");
    for &(i, idx) in &cases {
        let id = implementors[i].0;
        switch.push_str(&format!(" i32 {id}, label %{}", ctx.block_label(idx)));
    }
    switch.push_str(" ]");
    ctx.block().emit_raw(switch);
    ctx.block().mark_terminated();
    let mut answers = vec![("0".to_string(), start_l), ("0".to_string(), dispatch_l)];
    for (i, idx) in cases {
        ctx.current_block = idx;
        let (class_id, function) = implementors[i].clone();
        // The cased class's chain holders: one compare each, or nothing when
        // the program has no dynamic write or relink.
        let class = ctx
            .class_ids
            .iter()
            .find(|(_, id)| **id == class_id)
            .map(|(name, _)| name.clone());
        let holders_ok = match class {
            Some(class) => crate::proto_constants::holder_constant(ctx, &class, property),
            None => "false".to_string(),
        };
        let answer = if holders_ok == "true" {
            (i + 1).to_string()
        } else {
            ctx.block().select(
                crate::types::I1,
                &holders_ok,
                I32,
                &(i + 1).to_string(),
                "0",
            )
        };
        if crate::codegen::helpers::check_contracts() {
            crate::proto_constants::emit_selected_contract(
                ctx, recv, &answer, class_id, property, &function,
            );
        }
        let label = ctx.block().label.clone();
        ctx.block().br(&done_l);
        answers.push((answer, label));
    }
    ctx.current_block = done;
    let refs: Vec<(&str, &str)> = answers
        .iter()
        .map(|(v, l)| (v.as_str(), l.as_str()))
        .collect();
    ctx.block().phi(I32, &refs)
}
