//! Birth cost: a complete constructor-free construction is born as its image.
//!
//! A construction whose constructor is a complete run of `this.f = p` stores
//! (`ctor_prologue_stores`; every closed object literal is one) used to pay
//! for each slot three or four times: the nursery arm called the shared
//! helper `perry_birth_class`, whose two loops fill every slot with its
//! default, an I32 lane was then given its word of 0, and the prologue stores
//! overwrote every slot with its value. Here the nursery arm writes the
//! header image and the final word of every slot, once, with no call. The
//! shared helper stays the only other birth, at one call site: the
//! allocating arm takes it and then stores the same words (re-read from
//! their roots) through the same stores, and a construction whose values
//! fail the number test takes it and runs the constructor as before.
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
//!   #6969) and re-read from its root just before the bump (#12462). From
//!   the bump to the last slot store the image's two blocks (header, slots)
//!   hold address arithmetic and stores only: no call, so no safepoint,
//!   allocation or throw. A chunked literal (`Expr::LiteralChunks`) writes
//!   slots after its birth and is refused whole.
//! * The nursery arm is taken only when the bump fits, and the bump bound is
//!   zero whenever the live birth color is not: the image needs no color bit
//!   and no black-birth seed (the proof `inline_birth::fixed_class` uses).
//!   The allocating arm's object comes from the shared helper with its
//!   header, color and fill; only its slots are stored again.
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

/// One plan store as the image needs it again in the allocating arm.
struct Spec {
    slot: u32,
    arg_index: usize,
    addend: Option<f64>,
    /// An I32 lane of a class that is no object literal: the value is encoded.
    encode: bool,
}

/// The words the image writes, computed before the bump.
pub(in crate::lower_call) struct ImageValues<'g, 'a> {
    /// The I64 word of every plan store, in plan order.
    words: Vec<String>,
    specs: Vec<Spec>,
    /// The conjunction of the plain-finite-number tests the plan needs
    /// (`PrologueStore::requires_number_check`); `None` when none does.
    pub(in crate::lower_call) admit: Option<String>,
    class_id: u32,
    /// The arguments' roots: the allocating arm re-reads them after its call.
    group: &'g RootedGroup<'a>,
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

/// The plan's words from `args` (the arguments as re-read from their roots).
fn words(ctx: &mut FnCtx<'_>, specs: &[Spec], args: &[String]) -> Vec<String> {
    specs
        .iter()
        .map(|spec| {
            let arg = &args[spec.arg_index];
            let value = spec.addend.map_or_else(
                || arg.clone(),
                |addend| {
                    ctx.block()
                        .fadd(arg, &crate::nanbox::double_literal(addend))
                },
            );
            if spec.encode {
                crate::expr::rest_fields::encode(ctx, &value)
            } else {
                ctx.block().bitcast_double_to_i64(&value)
            }
        })
        .collect()
}

/// The image's words from the arguments re-read from their roots (always:
/// an earlier argument's lowering may have collected), and the number test
/// they need. Pure: no call between here and the bump.
pub(super) fn values<'g, 'a>(
    ctx: &mut FnCtx<'_>,
    group: &'g RootedGroup<'a>,
    class_name: &str,
    class: &perry_hir::Class,
    plan: &[PrologueStore],
    args: &[Expr],
    class_id: u32,
) -> Result<ImageValues<'g, 'a>> {
    let checked: Vec<bool> = plan
        .iter()
        .map(|store| store.requires_number_check(ctx, args))
        .collect();
    // I32 at rest (PR C): an I32 lane holds the value's I32 word. An
    // object-literal class's lane argument is its word already (`new.rs`);
    // any other class's value is encoded.
    let lanes = crate::expr::rest_fields::lanes_of(ctx, class_name);
    let specs: Vec<Spec> = plan
        .iter()
        .map(|store| Spec {
            slot: store.slot,
            arg_index: store.arg_index,
            addend: store.numeric_addend,
            encode: store.slot < 32 && lanes & (1 << store.slot) != 0 && !class.is_literal_shape(),
        })
        .collect();
    let reread = super::refresh_rooted_args(ctx, group)?;
    let mut admit: Option<String> = None;
    for (spec, _) in specs.iter().zip(&checked).filter(|(_, &check)| check) {
        let arg = &reread[spec.arg_index];
        let value = spec.addend.map_or_else(
            || arg.clone(),
            |addend| {
                ctx.block()
                    .fadd(arg, &crate::nanbox::double_literal(addend))
            },
        );
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(&value);
        let finite =
            crate::expr::class_field_inline_guard::emit_plain_finite_number_check(blk, &bits);
        admit = Some(match admit {
            Some(acc) => blk.and(I1, &acc, &finite),
            None => finite,
        });
    }
    let words = words(ctx, &specs, &reread);
    Ok(ImageValues {
        words,
        specs,
        admit,
        class_id,
        group,
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

// The shared helper's own constants: header 16 bytes, meta at +16, slots
// from +24 (`inline_birth::shared`); `decide` refuses any other target.
const BASE: u64 = 24;

/// The image's header, begun in the nursery arm; [`emit_slots`] finishes it.
pub(in crate::lower_call) struct Header {
    block: usize,
    start: usize,
    label: String,
    slots: usize,
}

/// Emit the image's header at `raw` in the current block (the nursery arm:
/// bump committed, color zero) and branch to the image's slots block.
pub(in crate::lower_call) fn emit_header(
    ctx: &mut FnCtx<'_>,
    raw: &str,
    header_image: &str,
    total_size: u64,
) -> Header {
    let capacity = (total_size.saturating_sub(BASE) / 8) as u32;
    let block = ctx.current_block;
    let start = ctx.block().insts().len();
    let blk = ctx.block();
    if crate::verify_facts::enabled() {
        for slot in 0..capacity {
            let p = blk.gep(I8, raw, &[(I64, &(BASE + u64::from(slot) * 8).to_string())]);
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
    let label = blk.label.clone();
    let slots = ctx.new_block("birth_image.slots");
    let slots_label = ctx.block_label(slots);
    ctx.block().br(&slots_label);
    Header {
        block,
        start,
        label,
        slots,
    }
}

/// In the allocating arm, after the shared birth produced `shared` (a
/// filled object with its header and color): re-read the arguments and join
/// the image's slots block, which stores every slot's final word for both
/// arms. A construction that failed the number test (`admit`) leaves for
/// `merge` with its fill, for the constructor. Returns each arm into `merge`
/// as `(handle, predecessor, written)`.
#[allow(clippy::too_many_arguments)]
pub(in crate::lower_call) fn emit_slots(
    ctx: &mut FnCtx<'_>,
    image: &ImageValues<'_, '_>,
    header: Header,
    raw: &str,
    shared: &str,
    rep: u64,
    total_size: u64,
    merge: &str,
) -> Result<Vec<(String, String, bool)>> {
    let capacity = (total_size.saturating_sub(BASE) / 8) as u32;
    let slots_label = ctx.block_label(header.slots);
    let mut arms = Vec::new();
    if let Some(admit) = &image.admit {
        let again = ctx.new_block("birth_image.again");
        let again_label = ctx.block_label(again);
        let pred = ctx.block().label.clone();
        ctx.block().cond_br(admit, &again_label, merge);
        arms.push((shared.to_string(), pred, false));
        ctx.current_block = again;
    }
    // The shared birth may have collected: the words are the re-read
    // arguments' (#6969).
    let reread = super::refresh_rooted_args(ctx, image.group)?;
    let again_words = words(ctx, &image.specs, &reread);
    let blk = ctx.block();
    let user = blk.inttoptr(I64, shared);
    let again_raw = blk.gep(I8, &user, &[(I64, "-8")]);
    let again_label = blk.label.clone();
    blk.br(&slots_label);

    ctx.current_block = header.slots;
    let verify = crate::verify_facts::enabled();
    let blk = ctx.block();
    let base = blk.phi(
        crate::types::PTR,
        &[(raw, &header.label), (&again_raw, &again_label)],
    );
    let mut slot_words: Vec<Option<String>> = vec![None; capacity as usize];
    for ((spec, first), second) in image.specs.iter().zip(&image.words).zip(&again_words) {
        assert!(
            spec.slot < capacity,
            "birth image: slot {} beyond capacity {capacity}",
            spec.slot
        );
        let word = blk.phi(I64, &[(first, &header.label), (second, &again_label)]);
        slot_words[spec.slot as usize] = Some(word);
    }
    let dropped = (verify && sabotaged("birthimage"))
        .then(|| image.specs.last().map(|spec| spec.slot))
        .flatten();
    let start = blk.insts().len();
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
        let p = blk.gep(
            I8,
            &base,
            &[(I64, &(BASE + u64::from(slot) * 8).to_string())],
        );
        // GC_STORE_AUDIT(INIT): final slot word of an unpublished image birth (idle-color nursery arm, or the shared birth's fresh object before any statepoint); no barrier or collection can be needed.
        blk.store(I64, &word, &p);
    }
    let end = blk.insts().len();
    let handle = {
        let user = blk.gep(I8, &base, &[(I64, "8")]);
        blk.ptrtoint(&user, I64)
    };
    let header_end = ctx.func.blocks()[header.block].insts().len();
    assert_window_has_no_call(ctx, header.block, header.start, header_end);
    assert_window_has_no_call(ctx, header.slots, start, end);
    if verify {
        witness(ctx, image.class_id, &base, capacity);
    }
    let pred = ctx.block().label.clone();
    ctx.block().br(merge);
    arms.push((handle, pred, true));
    Ok(arms)
}

/// Check mode: no slot still holds the "unwritten" mark; a slot that does
/// reports the site and exits 86.
fn witness(ctx: &mut FnCtx<'_>, cid: u32, raw: &str, capacity: u32) {
    crate::verify_facts::covered(
        crate::verify_facts::Fact::BirthImage,
        || serde_json::json!({ "class": cid, "slots": capacity }),
    );
    crate::verify_facts::tick(ctx);
    let blk = ctx.block();
    let mut any = "false".to_string();
    for slot in 0..capacity {
        let p = blk.gep(I8, raw, &[(I64, &(BASE + u64::from(slot) * 8).to_string())]);
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

/// The static half of the witness, in every build: the image's header (from
/// the bump) and its slots (to the last store) hold no call, so no
/// safepoint, allocation or throw.
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
