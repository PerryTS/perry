//! Class-nominated reads of a family's own data slots. The exact layout word
//! proves the slot mapping; the hole test proves presence at this read. No
//! canonical class-slot or raw-f64 clone capability is inferred from this arm.

use super::FnCtx;
use crate::layout_families::ProgramLayouts;
use crate::types::{DOUBLE, I1, I32, I64, I8};
use perry_hir::Expr;

const MAX_ARMS: usize = 8;

#[derive(Debug, PartialEq, Eq)]
struct Candidate {
    class_id: u32,
    variant: Option<usize>,
    slot: u32,
}

fn candidates(layouts: &ProgramLayouts, class_id: u32, property: &str) -> Vec<Candidate> {
    let descends = |mut id| {
        for _ in 0..64 {
            if id == class_id {
                return true;
            }
            match layouts.families.get(&id) {
                Some(f) if f.parent_class_id != 0 => id = f.parent_class_id,
                _ => return false,
            }
        }
        false
    };
    let mut arms = Vec::new();
    for f in layouts.families.values().filter(|f| descends(f.class_id)) {
        for (variant, keys) in std::iter::once((None, &f.keys)).chain(
            f.variants
                .iter()
                .enumerate()
                .map(|(i, v)| (Some(i), &v.keys)),
        ) {
            if let Some((_, slot)) = keys.iter().find(|(key, _)| key == property) {
                arms.push(Candidate {
                    class_id: f.class_id,
                    variant,
                    slot: *slot,
                });
                if arms.len() == MAX_ARMS {
                    return arms;
                }
            }
        }
    }
    arms
}

/// `PERRY_PROBE_VERIFY=1` while compiling: each compiled family slot access
/// also asks the runtime whether the receiver's record holds `property` at
/// `slot` (`perry_family_slot_verify`, counted by the run's
/// `PERRY_PROBE_VERIFY=1` summary). `receiver_bits` is the receiver as an
/// `i64` (a NaN-boxed pointer or a raw address).
pub(crate) fn emit_slot_verify(
    ctx: &mut FnCtx<'_>,
    receiver_bits: &str,
    property: &str,
    slot: u32,
) {
    if std::env::var("PERRY_PROBE_VERIFY").as_deref() != Ok("1") {
        return;
    }
    let key_idx = ctx.strings.intern(property);
    let key_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
    let blk = ctx.block();
    let key_box = blk.load(DOUBLE, &key_global);
    let key_bits = blk.bitcast_double_to_i64(&key_box);
    blk.call_void(
        "perry_family_slot_verify",
        &[
            (I64, receiver_bits),
            (I64, &key_bits),
            (I32, &slot.to_string()),
        ],
    );
}

pub(crate) struct FamilyRead {
    hits: Vec<(String, String)>,
    merge: usize,
}

/// The receiver has already been evaluated. On success, leave the caller on
/// the unchanged miss path and retain each present-slot hit for `finish`.
pub(crate) fn begin(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    receiver: &str,
) -> Option<FamilyRead> {
    if crate::target_layout::target_is_ilp32(ctx.target_triple)
        || !crate::layout_desc::layouts_enabled("executable", ctx.target_triple)
        || crate::expr::typed_feedback_emission_enabled()
        || ctx.is_static_class_this(object)
    {
        return None;
    }
    let class = crate::type_analysis::receiver_class_name(ctx, object)
        .or_else(|| super::property_get::guarded_declared_class_get_candidate(ctx, object));
    let class_id = if let Some(class) = class {
        *ctx.class_ids.get(&class)?
    } else if let perry_hir::Expr::LocalGet(id) = object {
        *ctx.receiver_families.get(id)?
    } else {
        return None;
    };
    let arms = candidates(&ctx.program_layouts, class_id, property);
    if arms.is_empty() {
        return None;
    }
    crate::expr::note_site("case2c-class-field");
    let word_idx = ctx.new_block("class_family.word");
    let miss_idx = ctx.new_block("class_family.miss");
    let merge = ctx.new_block("class_family.merge");
    let word_label = ctx.block_label(word_idx);
    let miss_label = ctx.block_label(miss_idx);
    let merge_label = ctx.block_label(merge);
    let bits = ctx.block().bitcast_double_to_i64(receiver);
    let recv = super::receiver_range::emit_fused_receiver_test(ctx.block(), &bits);
    ctx.block()
        .cond_br(&recv.is_object_pointer, &word_label, &miss_label);

    ctx.current_block = word_idx;
    let word_ptr = super::receiver_range::emit_field_ptr(ctx.block(), &recv.biased, 4);
    let word = ctx.block().load(I32, &word_ptr);
    let header = crate::target_layout::object_header_size_bytes(ctx.target_triple) as i64;
    let mut slots = std::collections::BTreeMap::<u32, Vec<Candidate>>::new();
    for arm in arms {
        slots.entry(arm.slot).or_default().push(arm);
    }
    let mut hits = Vec::new();
    for (slot, arms) in slots {
        let load = ctx.new_block("class_family.load");
        let next = ctx.new_block("class_family.next");
        let hit = ctx.new_block("class_family.present");
        let load_label = ctx.block_label(load);
        let next_label = ctx.block_label(next);
        let hit_label = ctx.block_label(hit);
        let mut accepted = "false".to_owned();
        for arm in arms {
            let desc = crate::layout_desc::note_family_variant_desc_use(arm.class_id, arm.variant);
            let blk = ctx.block();
            let ptr = blk.gep(
                I8,
                &desc,
                &[(I64, &crate::layout_desc::DESC_LAYOUT_ID_OFFSET.to_string())],
            );
            let expected = blk.load(I32, &ptr);
            // Only a registered ordinary-band layout proves an object. Reject
            // both 0 (declined registration) and u32::MAX (the startup marker).
            let relative = blk.sub(
                I32,
                &expected,
                &(crate::runtime_abi::SHAPE_ID_BASE as i32).to_string(),
            );
            let registered = blk.icmp_ult(I32, &relative, "1073741824");
            let equal = blk.icmp_eq(I32, &word, &expected);
            let equal = blk.and(I1, &registered, &equal);
            accepted = blk.or(I1, &accepted, &equal);
        }
        ctx.block().cond_br(&accepted, &load_label, &next_label);
        ctx.current_block = load;
        emit_slot_verify(ctx, &bits, property, slot);
        let ptr = super::receiver_range::emit_field_ptr(
            ctx.block(),
            &recv.biased,
            header + 8 * i64::from(slot),
        );
        let value = ctx.block().load(I64, &ptr);
        let present = ctx
            .block()
            .icmp_ne(I64, &value, crate::nanbox::TAG_HOLE_I64);
        // A hole can expose an inherited getter/value. Never answer undefined
        // here, and never reuse another field's presence check.
        ctx.block().cond_br(&present, &hit_label, &miss_label);
        ctx.current_block = hit;
        let value = ctx.block().bitcast_i64_to_double(&value);
        ctx.block().br(&merge_label);
        hits.push((value, hit_label));
        ctx.current_block = next;
    }
    ctx.block().br(&miss_label);
    ctx.current_block = miss_idx;
    Some(FamilyRead { hits, merge })
}

pub(crate) fn finish(ctx: &mut FnCtx<'_>, mut arm: FamilyRead, generic: &str) -> String {
    arm.hits
        .push((generic.to_owned(), ctx.block_label(ctx.current_block)));
    let merge_label = ctx.block_label(arm.merge);
    ctx.block().br(&merge_label);
    ctx.current_block = arm.merge;
    let values: Vec<_> = arm
        .hits
        .iter()
        .map(|(value, label)| (value.as_str(), label.as_str()))
        .collect();
    ctx.block().phi(DOUBLE, &values)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout_families::{FamilyLayout, FamilyVariant};

    fn family(id: u32, parent: u32, keys: &[(&str, u32)]) -> FamilyLayout {
        FamilyLayout {
            class_id: id,
            parent_class_id: parent,
            keys: keys
                .iter()
                .map(|(key, slot)| ((*key).into(), *slot))
                .collect(),
            slot_count: keys.len() as u32,
            own_count: keys.len() as u32,
            init_count: keys.len() as u32,
            discriminant: 0,
            variants: Vec::new(),
        }
    }

    #[test]
    fn family_slots_follow_keys_in_descendants_and_variants() {
        let mut layouts = ProgramLayouts::default();
        layouts
            .families
            .insert(1, family(1, 0, &[("a", 1), ("b", 0)]));
        let mut child = family(2, 1, &[("a", 1), ("b", 0), ("late", 2)]);
        child.own_count = 2;
        child.init_count = 2;
        child.variants.push(FamilyVariant {
            key_bits: 1,
            keys: vec![
                ("a".into(), 1),
                ("b".into(), 0),
                ("other".into(), 2),
                ("late".into(), 3),
            ],
            slot_count: 4,
        });
        layouts.families.insert(2, child);
        layouts.families.insert(3, family(3, 0, &[("late", 0)]));
        assert_eq!(
            candidates(&layouts, 1, "a"),
            vec![
                Candidate {
                    class_id: 1,
                    variant: None,
                    slot: 1
                },
                Candidate {
                    class_id: 2,
                    variant: None,
                    slot: 1
                },
                Candidate {
                    class_id: 2,
                    variant: Some(0),
                    slot: 1
                },
            ]
        );
        assert_eq!(
            candidates(&layouts, 1, "late"),
            vec![
                Candidate {
                    class_id: 2,
                    variant: None,
                    slot: 2
                },
                Candidate {
                    class_id: 2,
                    variant: Some(0),
                    slot: 3
                },
            ]
        );
        assert!(candidates(&layouts, 1, "absent").is_empty());
    }

    #[test]
    fn wide_or_cyclic_hierarchies_keep_bounded_guards() {
        let mut layouts = ProgramLayouts::default();
        for id in 1..30 {
            layouts
                .families
                .insert(id, family(id, if id == 1 { 0 } else { 1 }, &[("x", 0)]));
        }
        assert_eq!(candidates(&layouts, 1, "x").len(), MAX_ARMS);
        layouts.families.insert(50, family(50, 51, &[("x", 0)]));
        layouts.families.insert(51, family(51, 50, &[("x", 0)]));
        assert!(candidates(&layouts, 99, "x").is_empty());
    }
}
