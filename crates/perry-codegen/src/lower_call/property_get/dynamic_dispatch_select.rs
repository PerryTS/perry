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
/// directly (only a `cased` one), 0 for the generic arm, and the implementor
/// indices an answer can name (one per arm, see [`select_with_word`]).
pub(super) fn select(
    ctx: &mut FnCtx<'_>,
    recv: &str,
    property: &str,
    implementors: &[(u32, String)],
    cased: &[bool],
) -> (String, Vec<usize>) {
    let (word, admitted) =
        crate::lower_call::method_override::emit_direct_method_selection_probe(ctx, recv);
    select_with_word(ctx, recv, property, implementors, cased, &word, &admitted)
}

/// One arm of the class-id switch: the classes whose receivers call the same
/// body under the same proof. `rep` is the implementor an answer names.
struct Arm {
    rep: usize,
    function: String,
    /// The class declares the body itself (its arm may take a layout clone).
    own: Option<u32>,
    /// Chain classes still to prove (between the class and the shared
    /// suffix); `None`: no named chain, the arm is never taken.
    proof: Option<Vec<u32>>,
    class_ids: Vec<u32>,
}

/// Select from an already loaded receiver header word (`class_id | ShapeId
/// << 32`) and its admission predicate. An unadmitted word cannot select a
/// body. `recv` is read only by contract builds.
///
/// The switch has one arm per distinct (body, proof), not one per class:
/// classes that inherit the same body from holder H and need the same holder
/// proof (the classes between their own prototype and H) are case values of
/// one arm, with one proof and one answer. A class that declares the body
/// keeps its own arm (its call may be its layout clone). Returns the
/// selection and each arm's answer implementor.
pub(super) fn select_with_word(
    ctx: &mut FnCtx<'_>,
    recv: &str,
    property: &str,
    implementors: &[(u32, String)],
    cased: &[bool],
    word: &str,
    admitted: &str,
) -> (String, Vec<usize>) {
    let cid = ctx.block().trunc(I64, word, I32);
    let receiver_ok = crate::proto_constants::emit_receiver_unshadowed(ctx, word);
    let admitted = ctx.block().and(crate::types::I1, admitted, &receiver_ok);
    let class_of = |ctx: &FnCtx<'_>, id: u32| {
        ctx.class_ids
            .iter()
            .filter(|(_, cid)| **cid == id)
            .map(|(name, _)| name.clone())
            .min()
    };
    // The cased classes, one per class id (the first implementor wins).
    let mut seen_ids = std::collections::HashSet::new();
    let cases: Vec<usize> = implementors
        .iter()
        .enumerate()
        .filter(|&(i, (id, _))| cased[i] && seen_ids.insert(*id))
        .map(|(i, _)| i)
        .collect();
    let case_classes: Vec<(u32, String)> = cases
        .iter()
        .map(|&i| {
            let id = implementors[i].0;
            (id, class_of(ctx, id).unwrap_or_default())
        })
        .collect();
    // One holder proof for all cases: the receiver's own prototype by its
    // runtime class id and the chain suffix every case shares, once, here;
    // an arm proves only the classes between its own and that suffix.
    let shared = crate::proto_constants::shared_holder_proof(ctx, &cid, &case_classes, property);
    let (admitted, proofs): (String, Vec<Option<Vec<u32>>>) = match shared {
        Some((ok, extras)) => (ctx.block().and(crate::types::I1, &admitted, &ok), extras),
        // No shared proof (no shadow test, or ILP32): each arm proves its
        // class's whole chain.
        None => (
            admitted,
            case_classes
                .iter()
                .map(|(_, class)| crate::proto_constants::case_chain(ctx, class, property))
                .collect(),
        ),
    };
    let mut arms: Vec<Arm> = Vec::new();
    for ((&i, (id, class)), proof) in cases.iter().zip(&case_classes).zip(proofs) {
        let function = implementors[i].1.clone();
        let own = (ctx.methods.get(&(class.clone(), property.to_string())) == Some(&function))
            .then_some(*id);
        match arms.iter_mut().find(|a| {
            a.own.is_none() && own.is_none() && a.function == function && a.proof == proof
        }) {
            Some(arm) => arm.class_ids.push(*id),
            None => arms.push(Arm {
                rep: i,
                function,
                own,
                proof,
                class_ids: vec![*id],
            }),
        }
    }
    let done = ctx.new_block("pconst.selected");
    let done_l = ctx.block_label(done);
    let start_l = ctx.block().label.clone();
    let dispatch = ctx.new_block("pconst.admitted");
    let dispatch_l = ctx.block_label(dispatch);
    ctx.block().cond_br(&admitted, &dispatch_l, &done_l);
    ctx.current_block = dispatch;
    let blocks: Vec<usize> = arms.iter().map(|_| ctx.new_block("pconst.case")).collect();
    let mut switch = format!("switch i32 {cid}, label %{done_l} [");
    for (arm, &idx) in arms.iter().zip(&blocks) {
        let label = ctx.block_label(idx);
        for id in &arm.class_ids {
            switch.push_str(&format!(" i32 {id}, label %{label}"));
        }
    }
    switch.push_str(" ]");
    ctx.block().emit_raw(switch);
    ctx.block().mark_terminated();
    let mut answers = vec![("0".to_string(), start_l), ("0".to_string(), dispatch_l)];
    for (arm, idx) in arms.iter().zip(blocks) {
        ctx.current_block = idx;
        let holders_ok = match &arm.proof {
            None => "false".to_string(),
            Some(rest) => crate::proto_constants::case_holders_unshadowed(ctx, rest),
        };
        let answer = if holders_ok == "true" {
            (arm.rep + 1).to_string()
        } else {
            ctx.block().select(
                crate::types::I1,
                &holders_ok,
                I32,
                &(arm.rep + 1).to_string(),
                "0",
            )
        };
        if crate::codegen::helpers::check_contracts() {
            // A merged arm proves the receiver's runtime class; the witness
            // re-derives the body from it.
            let class_id = match arm.class_ids.as_slice() {
                [one] => one.to_string(),
                _ => cid.clone(),
            };
            crate::proto_constants::emit_selected_contract(
                ctx,
                recv,
                &answer,
                &class_id,
                property,
                &arm.function,
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
    (
        ctx.block().phi(I32, &refs),
        arms.iter().map(|a| a.rep).collect(),
    )
}
