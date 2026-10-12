//! Compact fixed-layout births. Color/overflow work stays in the shared helper.
use crate::expr::FnCtx;
use crate::types::{I64, I8};

/// Bound the replacement for the old header-store + helper-call sequence.
/// Larger layouts keep their shared fill instead of growing code at each site.
pub(crate) fn compact_slots(total_size: u64) -> Option<usize> {
    let slots = (total_size.checked_sub(24)? / 8) as usize;
    (total_size % 8 == 0 && (2..=4).contains(&slots)).then_some(slots)
}

/// The caller proved the live birth color is zero and committed the bump.
/// No call can collect between that proof and these unpublished stores.
pub(crate) fn fixed_class(
    ctx: &mut FnCtx<'_>,
    raw: &str,
    image: &str,
    rep: u64,
    slots: usize,
    fields: Option<&[(u32, String)]>,
) -> String {
    let blk = ctx.block();
    // GC_STORE_AUDIT(INIT): fresh idle-color header and shape.
    blk.emit_raw(format!("store <2 x i64> {image}, ptr {raw}, align 8"));
    let mut defaults: Vec<String> = (0..slots)
        .map(|slot| {
            if (rep >> (slot * 2)) & 3 == 1 {
                "0".into()
            } else {
                crate::nanbox::TAG_UNDEFINED_I64.into()
            }
        })
        .collect();
    if let Some(fields) = fields {
        for (slot, value) in fields {
            defaults[*slot as usize] = value.clone();
        }
    }
    let meta = blk.gep(I8, raw, &[(I64, "16")]);
    if slots == 3 {
        // GC_STORE_AUDIT(INIT): metadata and all three capacity slots, exactly
        // 32 bytes at +16 in a 48-byte object. One vector store on AVX targets.
        let mut values = vec!["0".to_string()];
        values.extend(defaults);
        store_vector(blk, &meta, &values);
    } else {
        // GC_STORE_AUDIT(INIT): fresh object metadata.
        blk.store(I64, "0", &meta);
        for (pair, values) in defaults.chunks(2).enumerate() {
            let address = blk.gep(I8, raw, &[(I64, &(24 + pair * 16).to_string())]);
            // GC_STORE_AUDIT(INIT): all capacity slots, including floor padding.
            if values.len() == 2 {
                store_vector(blk, &address, values);
            } else {
                blk.store(I64, &values[0].to_string(), &address);
            }
        }
    }
    let user = blk.gep(I8, raw, &[(I64, "8")]);
    blk.ptrtoint(&user, I64)
}

fn store_vector(blk: &mut crate::block::LlBlock, address: &str, values: &[String]) {
    let ty = format!("<{} x i64>", values.len());
    let vector = if values.iter().all(|value| !value.starts_with('%')) {
        format!(
            "<{}>",
            values
                .iter()
                .map(|value| format!("i64 {value}"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    } else {
        let mut vector = "poison".to_string();
        for (i, value) in values.iter().enumerate() {
            let next = blk.next_reg();
            blk.emit_raw(format!(
                "{next} = insertelement {ty} {vector}, i64 {value}, i32 {i}"
            ));
            vector = next;
        }
        vector
    };
    // GC_STORE_AUDIT(INIT): final field values plus every capacity default,
    // in the proven nursery/idle arm; no barrier or collection can be needed.
    blk.emit_raw(format!("store {ty} {vector}, ptr {address}, align 8"));
}

#[cfg(test)]
mod tests {
    #[test]
    fn compact_birth_budget_is_bounded_by_four_slots() {
        assert_eq!(super::compact_slots(40), Some(2));
        assert_eq!(super::compact_slots(48), Some(3));
        assert_eq!(super::compact_slots(56), Some(4));
        for size in [0, 24, 32, 41, 64, 16384] {
            assert_eq!(super::compact_slots(size), None);
        }
    }
}
