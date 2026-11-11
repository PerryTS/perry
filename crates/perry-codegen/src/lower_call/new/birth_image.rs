//! Birth cost: a complete constructor-free construction is born as its image.
//!
//! A construction whose constructor is a complete run of `this.f = p` stores
//! (`ctor_prologue_stores`; every closed object literal is one) used to pay
//! for each slot three or four times: the nursery arm called the shared
//! helper `perry_birth_class`, whose two loops fill every slot with its
//! default, an I32 lane was then given its word of 0, and the prologue stores
//! overwrote every slot with its value. Here the nursery arm writes the
//! header image and the final word of every slot, once, with no call. The
//! shared helper stays the only other birth: the allocating arm and a
//! construction whose values fail the number test take it (one call site),
//! and then run the constructor as before.
//!
//! ## Whose fact this is
//!
//! The birth shape (key set, slots, lanes) is the whole-program facts': the
//! class has a fixed layout, every declared field is a definite birth field
//! at codegen's own slot (`FactsField`: neither `declared` nor `reserved`),
//! and its I32 lanes are the facts' (`rest_fields::class_lanes`). The layout
//! itself is not restated here: the header image is the module's birth image
//! (`class_header_image_globals`, the ShapeId the runtime minted for
//! `class_birth_reps`), slot offsets come from `target_layout`, and a slot's
//! default is its birth rep's (`typed_shape`, `rest_fields`).
//!
//! ## Why no slot can be seen before it is written
//!
//! * Every argument is lowered and rooted before the allocation (`new.rs`,
//!   #6969) and re-read from its root just before the bump (#12462). The
//!   window from the bump to the last slot store holds address arithmetic
//!   and stores only: no call, so no safepoint, allocation or throw. A
//!   chunked literal (`Expr::LiteralChunks`) writes slots after its birth
//!   and is refused whole.
//! * The nursery arm is taken only when the bump fits, and the bump bound is
//!   zero whenever the live birth color is not: the image needs no color bit
//!   and no black-birth seed (the proof `inline_birth::fixed_class` uses).
//! * The handle is the merge phi after the stores.
//!
//! `PERRY_BIRTH_IMAGE=0` (also `off`, `false`) turns the image off (IR as
//! before). `PERRY_SITE_STATS=1` names every image and every refusal.
//! Check mode (`PERRY_VERIFY_FACTS=1`) marks every slot unwritten, writes the
//! image and reports a slot still marked (`Fact::BirthImage`, exit 86), and
//! asserts at compile time that the window holds no call. Negative controls:
//! `PERRY_FACTS_SABOTAGE=birthimage` (the image drops its last store) and
//! `birthwindow` (a call in the window: the compile panics).

use anyhow::Result;
use perry_hir::Expr;

use super::super::ctor_prologue_stores::{prologue_store_plan, PrologueStore};
use super::super::new_helpers::{ctor_chain_uses_new_target, local_constructor_symbol_exists};
use crate::expr::FnCtx;
use crate::rooting::RootedGroup;
use crate::types::{DOUBLE, I1, I32, I64, I8};

/// A NaN-box no value or lane default can be: the witness's "not written".
const UNWRITTEN: u64 = 0x7FFC_0000_B1A7_0001;

/// `PERRY_BIRTH_IMAGE=0` (`off`, `false`) turns the image off.
pub(crate) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_BIRTH_IMAGE").as_deref(),
            Ok("0" | "off" | "false")
        )
    })
}

/// The negative control `PERRY_FACTS_SABOTAGE=<arm>`.
fn sabotaged(arm: &str) -> bool {
    std::env::var("PERRY_FACTS_SABOTAGE").as_deref() == Ok(arm)
}

/// The words the image writes, computed before the bump.
pub(in crate::lower_call) struct ImageValues {
    /// `(slot, i64 word)` for every plan store, in plan order.
    pub(in crate::lower_call) values: Vec<(u32, String)>,
    /// The conjunction of the plain-finite-number tests the plan needs
    /// (`PrologueStore::requires_number_check`); `None` when none does.
    pub(in crate::lower_call) admit: Option<String>,
    pub(in crate::lower_call) class_id: u32,
}

/// The one predicate: `Ok(plan)` when this `new` is born as an image, else
/// the reason it is not.
#[allow(clippy::too_many_arguments)]
fn decide(
    ctx: &FnCtx<'_>,
    class_name: &str,
    class: &perry_hir::Class,
    args: &[Expr],
    lowered_arg_count: usize,
    layout_ready: bool,
) -> std::result::Result<Vec<PrologueStore>, &'static str> {
    if std::env::var_os("PERRY_INLINE_CTOR").is_some() {
        return Err("inline constructors forced");
    }
    if !layout_ready {
        return Err("layout not declared at allocation");
    }
    if !local_constructor_symbol_exists(ctx, class) || ctor_chain_uses_new_target(ctx, class) {
        return Err("no plain local constructor");
    }
    if crate::expr::literal_chunks::held_len(args) != args.len() {
        return Err("chunked literal");
    }
    if crate::target_layout::target_is_ilp32(ctx.target_triple)
        || crate::target_layout::object_slot_base_bytes(
            ctx.target_triple,
            crate::target_layout::class_instance_form(class_name),
        ) != 16
    {
        return Err("target layout");
    }
    if ctx.wpfacts_off {
        return Err("no facts");
    }
    // The post-birth diamond's `constructor_layout_ready` is
    // `constructor_stores_ready && layout_ready`; the inline arm sets
    // `constructor_stores_ready` exactly when the keys are not deferred,
    // which the nursery arm requires before it takes an image.
    let Some(plan) = prologue_store_plan(ctx, class_name, class, lowered_arg_count, true) else {
        return Err("no complete store plan");
    };
    if plan.iter().any(|store| store.family_key.is_some()) {
        return Err("family layout");
    }
    let Some(&cid) = ctx.class_ids.get(class_name) else {
        return Err("no class id");
    };
    let Some(facts) = ctx.wpfacts.classes.get(&cid) else {
        return Err("no class facts");
    };
    if !facts.fixed_layout {
        return Err("layout not fixed");
    }
    for field in &class.fields {
        let Some(f) = ctx.wpfacts.field(cid, &field.name) else {
            return Err("field without a fact");
        };
        let slot = crate::type_analysis::class_field_global_index(ctx, class_name, &field.name);
        if f.declared || f.reserved {
            return Err("field not definite at birth");
        }
        if slot != Some(f.slot) {
            return Err("slot disagreement");
        }
    }
    let mut slots: Vec<u32> = plan.iter().map(|store| store.slot).collect();
    slots.sort_unstable();
    slots.dedup();
    if slots.len() != plan.len() || slots.len() != class.fields.len() {
        return Err("plan does not cover the fields once");
    }
    Ok(plan)
}

/// [`decide`], with the probe line.
pub(super) fn plan(
    ctx: &FnCtx<'_>,
    class_name: &str,
    class: &perry_hir::Class,
    args: &[Expr],
    lowered_arg_count: usize,
    layout_ready: bool,
) -> Option<Vec<PrologueStore>> {
    if !enabled() {
        return None;
    }
    match decide(
        ctx,
        class_name,
        class,
        args,
        lowered_arg_count,
        layout_ready,
    ) {
        Ok(plan) => {
            crate::expr::note_site(&format!(
                "birth-image {class_name} slots={} {}",
                plan.len(),
                ctx.func.name
            ));
            Some(plan)
        }
        Err(why) => {
            crate::expr::note_site(&format!(
                "birth-image-refused {class_name} why={} {}",
                why.replace(' ', "-"),
                ctx.func.name
            ));
            None
        }
    }
}

/// The image's words from the arguments re-read from their roots (always:
/// an earlier argument's lowering may have collected), and the number test
/// they need. Pure: no call between here and the bump.
pub(super) fn values(
    ctx: &mut FnCtx<'_>,
    group: &RootedGroup<'_>,
    class_name: &str,
    class: &perry_hir::Class,
    plan: &[PrologueStore],
    args: &[Expr],
    class_id: u32,
) -> Result<ImageValues> {
    let checked: Vec<bool> = plan
        .iter()
        .map(|store| store.requires_number_check(ctx, args))
        .collect();
    let lanes = crate::expr::rest_fields::lanes_of(ctx, class_name);
    let reread = super::refresh_rooted_args(ctx, group)?;
    let mut admit: Option<String> = None;
    let mut values = Vec::with_capacity(plan.len());
    for (store, &check) in plan.iter().zip(&checked) {
        let arg = &reread[store.arg_index];
        let value = store.numeric_addend.map_or_else(
            || arg.clone(),
            |addend| {
                ctx.block()
                    .fadd(arg, &crate::nanbox::double_literal(addend))
            },
        );
        if check {
            let blk = ctx.block();
            let bits = blk.bitcast_double_to_i64(&value);
            let finite =
                crate::expr::class_field_inline_guard::emit_plain_finite_number_check(blk, &bits);
            admit = Some(match admit {
                Some(acc) => blk.and(I1, &acc, &finite),
                None => finite,
            });
        }
        // I32 at rest (PR C): an I32 lane holds the value's I32 word. An
        // object-literal class's lane argument is its word already
        // (`new.rs`); any other class's value is encoded.
        let lane = store.slot < 32 && lanes & (1 << store.slot) != 0;
        let word = if lane && !class.is_literal_shape() {
            crate::expr::rest_fields::encode(ctx, &value)
        } else {
            ctx.block().bitcast_double_to_i64(&value)
        };
        values.push((store.slot, word));
    }
    Ok(ImageValues {
        values,
        admit,
        class_id,
    })
}

/// The default word of a slot no plan store writes (capacity past the
/// declared fields): what the shared birth leaves there.
fn default_word(rep: u64, slot: u32) -> String {
    if slot < 32 && crate::expr::rest_fields::lanes_of_rep(rep) & (1 << slot) != 0 {
        crate::expr::rest_fields::zero_word().into()
    } else if crate::typed_shape::birth_rep_slot_is_f64(rep, slot) {
        "0".into()
    } else {
        crate::nanbox::TAG_UNDEFINED_I64.into()
    }
}

/// Emit the image at `raw` in the current block (bump committed, color
/// zero). Returns the handle; the caller branches to its merge.
pub(in crate::lower_call) fn emit(
    ctx: &mut FnCtx<'_>,
    image: &ImageValues,
    raw: &str,
    header_image: &str,
    rep: u64,
    total_size: u64,
) -> String {
    // The shared helper's own constants: header 16 bytes, meta at +16, slots
    // from +24 (`inline_birth::shared`); `decide` refuses any other target.
    const BASE: u64 = 24;
    let capacity = (total_size.saturating_sub(BASE) / 8) as u32;
    let verify = crate::verify_facts::enabled();
    let mut slot_words: Vec<Option<String>> = vec![None; capacity as usize];
    let dropped = (verify && sabotaged("birthimage"))
        .then(|| image.values.last().map(|(slot, _)| *slot))
        .flatten();
    for (slot, word) in &image.values {
        assert!(
            *slot < capacity,
            "birth image: slot {slot} beyond capacity {capacity}"
        );
        slot_words[*slot as usize] = Some(word.clone());
    }
    let window_block = ctx.current_block;
    let window_start = ctx.block().insts().len();
    let blk = ctx.block();
    let slot_ptr = |blk: &mut crate::block::LlBlock, slot: u32| {
        blk.gep(I8, raw, &[(I64, &(BASE + u64::from(slot) * 8).to_string())])
    };
    if verify {
        for slot in 0..capacity {
            let p = slot_ptr(blk, slot);
            // GC_STORE_AUDIT(INIT): check-mode "unwritten" mark in an unpublished idle-color birth.
            blk.store(I64, &UNWRITTEN.to_string(), &p);
        }
    }
    // GC_STORE_AUDIT(INIT): fresh idle-color header and shape of an image birth.
    blk.emit_raw(format!(
        "store <2 x i64> {header_image}, ptr {raw}, align 8"
    ));
    let meta = blk.gep(I8, raw, &[(I64, "16")]);
    // GC_STORE_AUDIT(INIT): fresh object metadata of an image birth.
    blk.store(I64, "0", &meta);
    for slot in 0..capacity {
        if Some(slot) == dropped {
            continue;
        }
        if verify && sabotaged("birthwindow") && slot + 1 == capacity {
            blk.call_void("perry_verify_facts_tick", &[]);
        }
        let word = slot_words[slot as usize]
            .clone()
            .unwrap_or_else(|| default_word(rep, slot));
        let p = slot_ptr(blk, slot);
        // GC_STORE_AUDIT(INIT): final slot word of an unpublished idle-color image birth; no barrier or collection can be needed.
        blk.store(I64, &word, &p);
    }
    let window_end = blk.insts().len();
    let handle = {
        let user = blk.gep(I8, raw, &[(I64, "8")]);
        blk.ptrtoint(&user, I64)
    };
    assert_window_has_no_call(ctx, window_block, window_start, window_end);
    if verify {
        witness(ctx, image, raw, BASE, capacity);
    }
    handle
}

/// Check mode: no slot still holds the "unwritten" mark; a slot that does
/// reports the site and exits 86.
fn witness(ctx: &mut FnCtx<'_>, image: &ImageValues, raw: &str, base: u64, capacity: u32) {
    let cid = image.class_id;
    crate::verify_facts::covered(
        crate::verify_facts::Fact::BirthImage,
        || serde_json::json!({ "class": cid, "slots": capacity }),
    );
    crate::verify_facts::tick(ctx);
    let blk = ctx.block();
    let mut any = "false".to_string();
    for slot in 0..capacity {
        let p = blk.gep(I8, raw, &[(I64, &(base + u64::from(slot) * 8).to_string())]);
        let v = blk.load(I64, &p);
        let unwritten = blk.icmp_eq(I64, &v, &UNWRITTEN.to_string());
        any = blk.or(I1, &any, &unwritten);
    }
    let fail_idx = ctx.new_block("birth_image.unwritten");
    let ok_idx = ctx.new_block("birth_image.witnessed");
    let fail_label = ctx.block_label(fail_idx);
    let ok_label = ctx.block_label(ok_idx);
    ctx.block().cond_br(&any, &fail_label, &ok_label);
    ctx.current_block = fail_idx;
    let what = format!("birth image of class {cid} leaves a slot of {capacity} unwritten");
    let site =
        crate::verify_facts::site_arg(ctx, &what, crate::verify_facts::Fact::BirthImage, &[cid]);
    let mark = crate::nanbox::double_literal(f64::from_bits(UNWRITTEN));
    let _ = crate::verify_facts::check(
        ctx,
        I32,
        "perry_wpfacts_check_miss",
        &[(DOUBLE, &mark)],
        &site,
    );
    ctx.block().unreachable();
    ctx.current_block = ok_idx;
}

/// The static half of the witness, in every build: from the bump to the last
/// slot store the image's block holds no call (so no safepoint, allocation
/// or throw).
fn assert_window_has_no_call(ctx: &FnCtx<'_>, block: usize, start: usize, end: usize) {
    let insts = ctx.func.blocks()[block].insts();
    let mut line = String::new();
    for inst in &insts[start..end] {
        line.clear();
        inst.render_into(&mut line);
        let text = line.trim();
        assert!(
            !(text.contains(" call ") || text.starts_with("call ") || text.contains("invoke ")),
            "birth image: a call before the last slot store: {text}"
        );
    }
}
