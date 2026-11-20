//! Two-tier public-entry dispatch for typed and specialized clones.
//!
//! A public entry decides per call whether its arguments satisfy the clone's
//! representation contract. For a Number lane that contract admits the plain
//! double, the one encoding a Number has, which is also what the clone wants.
//!
//! Tier 1 admits ordered doubles only (`fcmp ord`, which x86-64 lowers to
//! `ucomisd; jnp`) and passes them through untouched. Every boxed tag is a NaN
//! pattern, so an ordered value is always a plain number and no bit test is
//! needed on the common path. Only a NaN-patterned argument reaches tier 2,
//! which runs the sign-closed plain-number test (canonical and payload NaNs
//! that are not tags). Everything else takes the unchanged generic body. The
//! routing decision per value is identical to the one-tier form.
//!
//! Tier 1 is a real branch, not a predicate folded into tier 2: LLVM rewrites
//! `or(fcmp ord, masked compare)` and the shift/rotate forms of the
//! sign-closed test back into the masked compare, which costs two more
//! instructions per Number argument than the ordered branch.

use crate::block::LlBlock;
use crate::function::LlFunction;
use crate::types::{DOUBLE, I1, I64};

/// Largest bit pattern (as a signed i64) that is a plain JS Number. Perry's
/// own tags occupy the positive-qNaN top words `0x7FF9..=0x7FFF`; every other
/// pattern, including negative values and canonical NaN, is a double.
pub(crate) const PLAIN_NUMBER_MAX_I64: &str = "9221401712017801215"; // 0x7FF8_FFFF_FFFF_FFFF

/// Entry contract for one argument.
#[derive(Debug, Clone, Default)]
pub(crate) struct EntryArgGuard {
    /// Number lane: ordered doubles pass in tier 1, NaN Numbers in tier 2.
    pub number: bool,
    /// Any other exact predicate (typed leaf guard or descriptor validator),
    /// already emitted in the entry block.
    pub exact: Option<String>,
}

/// The sign-closed numeric proof: magnitude below the tag band. Raw numeric
/// ingress canonicalizes payload NaNs; the mirror excludes native HANDLE_TAG
/// and prevents arithmetic from turning an admitted negative NaN into a tag.
pub(crate) fn emit_plain_number_test(blk: &mut LlBlock, value: &str) -> String {
    let bits = blk.bitcast_double_to_i64(value);
    emit_plain_number_bits_test(blk, &bits)
}

/// [`emit_plain_number_test`] on a word already in an i64 register. The one
/// emitted form of `JSValue::is_number`: a one-sided signed compare against
/// the positive tag band admits `HANDLE_TAG` (0xFFF9, a negative NaN) as a
/// "number", and an `fcmp` on it then answers as if for NaN (#12540).
pub(crate) fn emit_plain_number_bits_test(blk: &mut LlBlock, bits: &str) -> String {
    let magnitude = blk.and(I64, bits, "9223372036854775807");
    blk.icmp_ule(I64, &magnitude, PLAIN_NUMBER_MAX_I64)
}

/// Tier-1 Number test: the value is not a NaN pattern. Every tag (including
/// HANDLE_TAG in negative-NaN space) is a NaN, so this alone proves a plain
/// number; NaN-patterned numbers are left to [`emit_plain_number_test`].
pub(crate) fn emit_ordered_number_test(blk: &mut LlBlock, value: &str) -> String {
    blk.fcmp("ord", value, "0.0")
}

/// Entry contract for a typed-ABI lane: Number lanes use the two tiers, the
/// other representations keep their exact inline predicate.
pub(crate) fn typed_entry_arg_guard(
    blk: &mut LlBlock,
    rep: super::typed_abi::TypedParamRep,
    arg: &str,
) -> EntryArgGuard {
    match rep {
        super::typed_abi::TypedParamRep::F64 => EntryArgGuard {
            number: true,
            exact: None,
        },
        _ => EntryArgGuard {
            number: false,
            exact: Some(super::typed_abi::emit_typed_arg_guard(blk, rep, arg)),
        },
    }
}

fn and_all(blk: &mut LlBlock, parts: Vec<String>) -> Option<String> {
    let mut acc: Option<String> = None;
    for part in parts {
        acc = Some(match acc {
            Some(prev) => blk.and(I1, &prev, &part),
            None => part,
        });
    }
    acc
}

/// Emit the dispatch starting in block 0 of `wf`, which must be unterminated
/// and already hold every `exact` guard and `extra_guard`.
///
/// `fast` receives the arguments. Both tiers branch to its one block, so the
/// clone is called from exactly one site and stays inlinable. It returns the
/// value to return.
pub(crate) fn emit_tiered_entry_dispatch(
    wf: &mut LlFunction,
    label_prefix: &str,
    args: &[String],
    guards: &[EntryArgGuard],
    extra_guard: Option<String>,
    fast: &mut dyn FnMut(&mut LlBlock, &[String]) -> String,
    fallback: &mut dyn FnMut(&mut LlBlock) -> String,
) {
    debug_assert_eq!(args.len(), guards.len());
    let any_number = guards.iter().any(|g| g.number);

    let tier1_guard = {
        let blk = wf.block_mut(0).unwrap();
        let mut parts = Vec::new();
        for (arg, guard) in args.iter().zip(guards) {
            if guard.number {
                parts.push(emit_ordered_number_test(blk, arg));
            }
            if let Some(exact) = &guard.exact {
                parts.push(exact.clone());
            }
        }
        if let Some(extra) = &extra_guard {
            parts.push(extra.clone());
        }
        and_all(blk, parts)
    };

    let Some(tier1_guard) = tier1_guard else {
        let blk = wf.block_mut(0).unwrap();
        let value = fast(blk, args);
        blk.ret(DOUBLE, &value);
        return;
    };

    let fast_idx = wf.num_blocks();
    let fast_label = wf
        .create_block(&format!("{label_prefix}.fast"))
        .label
        .clone();
    let tier2 = any_number.then(|| {
        let idx = wf.num_blocks();
        let label = wf
            .create_block(&format!("{label_prefix}.nan"))
            .label
            .clone();
        (idx, label)
    });
    let fallback_idx = wf.num_blocks();
    let fallback_label = wf
        .create_block(&format!("{label_prefix}.fallback"))
        .label
        .clone();

    let miss_label = tier2
        .as_ref()
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| fallback_label.clone());
    wf.block_mut(0)
        .unwrap()
        .cond_br(&tier1_guard, &fast_label, &miss_label);

    // Tier 2: some Number argument is a NaN pattern. Admit the plain ones
    // (canonical or payload NaN outside the tag band, either sign); they are
    // already the double the clone wants.
    if let Some((idx, _)) = tier2 {
        let blk = wf.block_mut(idx).unwrap();
        let mut parts = Vec::new();
        for (arg, guard) in args.iter().zip(guards) {
            if guard.number {
                parts.push(emit_plain_number_test(blk, arg));
            }
            if let Some(exact) = &guard.exact {
                parts.push(exact.clone());
            }
        }
        if let Some(extra) = &extra_guard {
            parts.push(extra.clone());
        }
        let ok = and_all(blk, parts).expect("tier 2 has at least one number lane");
        blk.cond_br(&ok, &fast_label, &fallback_label);
    }

    {
        let blk = wf.block_mut(fast_idx).unwrap();
        let value = fast(blk, args);
        blk.ret(DOUBLE, &value);
    }

    let blk = wf.block_mut(fallback_idx).unwrap();
    let value = fallback(blk);
    blk.ret(DOUBLE, &value);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_number_bound_is_the_last_untagged_pattern() {
        let bound: i64 = PLAIN_NUMBER_MAX_I64.parse().unwrap();
        assert_eq!(bound as u64, 0x7FF8_FFFF_FFFF_FFFF);
        assert_eq!(bound as u64 + 1, crate::nanbox::SHORT_STRING_TAG);
        // Canonical NaN and negative values sit below the bound as signed i64.
        assert!((f64::NAN.to_bits() as i64) <= bound);
        assert!(((-0.0f64).to_bits() as i64) <= bound);
        assert!(((-1.5f64).to_bits() as i64) <= bound);
        assert!((f64::INFINITY.to_bits() as i64) <= bound);
        for tag in [
            crate::nanbox::INT32_TAG,
            crate::nanbox::POINTER_TAG,
            crate::nanbox::STRING_TAG,
            crate::nanbox::BIGINT_TAG,
            crate::nanbox::TAG_UNDEFINED,
            crate::nanbox::TAG_TRUE,
        ] {
            assert!((tag as i64) > bound, "{tag:#x}");
        }
    }
}
