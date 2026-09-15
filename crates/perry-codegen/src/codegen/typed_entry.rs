//! Two-tier public-entry dispatch for typed and specialized clones.
//!
//! A public entry decides per call whether its arguments satisfy the clone's
//! representation contract. For a Number lane that contract admits both plain
//! doubles and INT32-tagged boxes, and the clone wants a plain double. Deciding
//! both in one predicate and normalizing unconditionally made every call pay
//! the rare int32 case first: `addNum(a, b)` ran a 13-instruction combined
//! guard plus two `scvtf`/`fcsel` normalizations before its `fadd`.
//!
//! Tier 1 admits plain doubles only (`bits <=s 0x7FF8_FFFF_FFFF_FFFF`, one
//! compare per argument) and passes them through untouched. Only when a Number
//! argument is not a plain double does tier 2 test for an int32 box and pass
//! the converted double. Everything else takes the unchanged generic body.
//! The routing decision per value is identical to the one-tier form.

use crate::block::LlBlock;
use crate::function::LlFunction;
use crate::types::{DOUBLE, I1, I32, I64};

/// Largest bit pattern (as a signed i64) that is a plain JS Number. Perry's
/// own tags occupy the positive-qNaN top words `0x7FF9..=0x7FFF`; every other
/// pattern, including negative values and canonical NaN, is a double.
pub(crate) const PLAIN_NUMBER_MAX_I64: &str = "9221401712017801215"; // 0x7FF8_FFFF_FFFF_FFFF

/// Entry contract for one argument.
#[derive(Debug, Clone, Default)]
pub(crate) struct EntryArgGuard {
    /// Number lane: plain doubles pass in tier 1, int32 boxes are converted to
    /// the equal double in tier 2.
    pub number: bool,
    /// Any other exact predicate (typed leaf guard or descriptor validator),
    /// already emitted in the entry block.
    pub exact: Option<String>,
}

/// `bits <=s PLAIN_NUMBER_MAX` — `JSValue::is_number` for one value.
pub(crate) fn emit_plain_number_test(blk: &mut LlBlock, value: &str) -> String {
    let bits = blk.bitcast_double_to_i64(value);
    blk.icmp_sle(I64, &bits, PLAIN_NUMBER_MAX_I64)
}

/// `JSValue::is_int32` — the INT32 tag with any 32-bit payload.
pub(crate) fn emit_int32_box_test(blk: &mut LlBlock, value: &str) -> String {
    let bits = blk.bitcast_double_to_i64(value);
    let identity_mask = crate::nanbox::i64_literal(!crate::nanbox::INT32_MASK);
    let identity = blk.and(I64, &bits, &identity_mask);
    blk.icmp_eq(I64, &identity, crate::nanbox::INT32_TAG_I64)
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

/// Raw clone argument for a value that passed [`emit_tiered_entry_dispatch`]:
/// a Number lane already is the plain double the clone wants.
pub(crate) fn emit_typed_arg_to_raw_after_entry_tier(
    blk: &mut LlBlock,
    rep: super::typed_abi::TypedParamRep,
    value: &str,
) -> String {
    match rep {
        super::typed_abi::TypedParamRep::F64 => value.to_string(),
        _ => super::typed_abi::emit_typed_arg_to_raw(blk, rep, value),
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
/// `fast` receives one value per argument: the original argument, or for a
/// Number lane the plain double (tier 1) / converted int32 box (tier 2), merged
/// by a phi so the clone is called from exactly one site and stays inlinable.
/// It returns the value to return.
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

    let (entry_label, plain_tests, tier1_guard) = {
        let blk = wf.block_mut(0).unwrap();
        let mut plain_tests: Vec<Option<String>> = Vec::with_capacity(args.len());
        let mut parts = Vec::new();
        for (arg, guard) in args.iter().zip(guards) {
            if guard.number {
                let plain = emit_plain_number_test(blk, arg);
                parts.push(plain.clone());
                plain_tests.push(Some(plain));
            } else {
                plain_tests.push(None);
            }
            if let Some(exact) = &guard.exact {
                parts.push(exact.clone());
            }
        }
        if let Some(extra) = &extra_guard {
            parts.push(extra.clone());
        }
        let tier1 = and_all(blk, parts);
        (blk.label.clone(), plain_tests, tier1)
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
    let tier2 = if any_number {
        let idx = wf.num_blocks();
        let label = wf
            .create_block(&format!("{label_prefix}.int32"))
            .label
            .clone();
        let norm_idx = wf.num_blocks();
        let norm_label = wf
            .create_block(&format!("{label_prefix}.int32_convert"))
            .label
            .clone();
        Some((idx, label, norm_idx, norm_label))
    } else {
        None
    };
    let fallback_idx = wf.num_blocks();
    let fallback_label = wf
        .create_block(&format!("{label_prefix}.fallback"))
        .label
        .clone();

    let miss_label = tier2
        .as_ref()
        .map(|(_, label, _, _)| label.clone())
        .unwrap_or_else(|| fallback_label.clone());
    wf.block_mut(0)
        .unwrap()
        .cond_br(&tier1_guard, &fast_label, &miss_label);

    // Tier 2: an int32 box where a plain double was expected. Convert and
    // join the fast block.
    let converted: Option<(String, Vec<String>)> = tier2.map(|(idx, _, norm_idx, norm_label)| {
        let mut int32_tests: Vec<Option<String>> = Vec::with_capacity(args.len());
        {
            let blk = wf.block_mut(idx).unwrap();
            let mut parts = Vec::new();
            for ((arg, guard), plain) in args.iter().zip(guards).zip(&plain_tests) {
                if guard.number {
                    let int32 = emit_int32_box_test(blk, arg);
                    let plain = plain.as_ref().expect("number lane has a plain test");
                    parts.push(blk.or(I1, plain, &int32));
                    int32_tests.push(Some(int32));
                } else {
                    int32_tests.push(None);
                }
                if let Some(exact) = &guard.exact {
                    parts.push(exact.clone());
                }
            }
            if let Some(extra) = &extra_guard {
                parts.push(extra.clone());
            }
            let ok = and_all(blk, parts).expect("tier 2 has at least one number lane");
            blk.cond_br(&ok, &norm_label, &fallback_label);
        }
        let blk = wf.block_mut(norm_idx).unwrap();
        let values: Vec<String> = args
            .iter()
            .zip(&int32_tests)
            .map(|(arg, int32)| match int32 {
                Some(is_int32) => {
                    let bits = blk.bitcast_double_to_i64(arg);
                    let low = blk.trunc(I64, &bits, I32);
                    let converted = blk.sitofp(I32, &low, DOUBLE);
                    blk.select(I1, is_int32, DOUBLE, &converted, arg)
                }
                None => arg.clone(),
            })
            .collect();
        blk.br(&fast_label);
        (norm_label, values)
    });

    {
        let blk = wf.block_mut(fast_idx).unwrap();
        let values: Vec<String> = match &converted {
            Some((norm_label, converted_values)) => args
                .iter()
                .zip(guards)
                .zip(converted_values)
                .map(|((arg, guard), conv)| {
                    if guard.number {
                        blk.phi(DOUBLE, &[(arg, &entry_label), (conv, norm_label)])
                    } else {
                        arg.clone()
                    }
                })
                .collect(),
            None => args.to_vec(),
        };
        let value = fast(blk, &values);
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
