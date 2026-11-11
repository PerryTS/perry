//! Fill births: the fused fills `new Array(k).fill(v)` codegen may bear as
//! one birth, with the length each is born with.
//!
//! A fused fill (`Gen::call_with`, DESIGN-p4l1 §2.6) is a one-argument
//! `fill` called on the fresh `new Array(<one argument>)` of the builtin
//! `Array`: the receiver has no binding and no own `fill`, its prototype is
//! `%Array.prototype%`, and the intrinsic lock refuses any Replace, Delete or
//! Redefine of that image key, so the call is the native `fill` and writes
//! every index below the length. When the constructor's argument is the
//! node of an integer literal `k` the runtime backs densely
//! (`SiteBirth::len`: `0 <= k <= MAX_FRESH_DENSE_ARRAY_LENGTH`), the two
//! calls together make an Array of length `k` whose every element is the
//! fill value, and nothing can observe the Array between them: the
//! constructor cannot throw, and the fill's argument is evaluated after it,
//! without a name for the Array.
//!
//! The length is a fact about the argument's value, not about its
//! expression: `new Array((f(), 8))` has the literal's node too. Codegen
//! therefore bears the birth only when the argument is the literal itself
//! (`perry-codegen` `fill_birth::plan`), so no effect is dropped.
//!
//! **Export** (`export`): per module, the `fill` call's offset (codegen's
//! key for the call, as `rest_births`) with `k`. Codegen bears such a call as
//! one birth (`perry-codegen` `expr/fill_birth.rs`): one allocation with the
//! final length and capacity, each element stored once in its final word. The
//! element kind and the capacity are the at-rest facts' (`rest_births`,
//! `rest_reserves`); a birth that is not at rest stores the value's word.
//!
//! Not exported: a constructor argument that is no integer literal (it may
//! throw a RangeError before the fill's argument is evaluated, or name one
//! element), a longer `new Array(k)` (a length over an empty store), a
//! `fill` with a start or an end, a spread, a program class or binding named
//! `Array` (no fused fill: `gen_call::fused_fill_receiver`), a call codegen
//! cannot key (offset 0), an offset two fills with different lengths share,
//! and everything when the solve aborted or ran per unit.
//!
//! `PERRY_ARRAY_FILL_BIRTH=0` (also `off`, `false`) exports nothing: the
//! two-step birth, as before.

use crate::bindings::ModuleBindings;
use crate::model::Model;

/// `PERRY_ARRAY_FILL_BIRTH=0` turns the fill birth off.
pub(crate) fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_ARRAY_FILL_BIRTH").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// Every fill birth (module docs): `(module, offset, k)`, sorted, one per
/// offset.
pub(crate) fn births(m: &Model) -> Vec<(u32, u32, u32)> {
    if !m.fill_births || !m.array_reps || m.aborted || m.unit_mode {
        return Vec::new();
    }
    let dense = i64::from(perry_abi::MAX_FRESH_DENSE_ARRAY_LENGTH);
    let mut out: Vec<(u32, u32, u32)> = m
        .rest_fills
        .iter()
        .filter(|f| f.offset != 0)
        .filter_map(|f| {
            let birth = m.arr_len_at_site.get(&f.site)?;
            let k = birth.len.filter(|k| (0..=dense).contains(k))?;
            birth.hole_free.then_some((f.module, f.offset, k as u32))
        })
        .collect();
    out.sort_unstable();
    out.dedup();
    // One length per call: an offset with two is no birth.
    let clash: Vec<(u32, u32)> = out
        .windows(2)
        .filter(|w| (w[0].0, w[0].1) == (w[1].0, w[1].1))
        .map(|w| (w[0].0, w[0].1))
        .collect();
    out.retain(|&(mi, offset, _)| !clash.contains(&(mi, offset)));
    out
}

/// Fill `ModuleBindings::fill_births` (module docs).
pub(crate) fn export(m: &Model, bindings: &mut [ModuleBindings]) {
    for (mi, offset, k) in births(m) {
        if let Some(mb) = bindings.get_mut(mi as usize) {
            mb.fill_births.push((offset, k));
        }
    }
}

/// The probe (`PERRY_FACTS_DUMP`): one `# fill birth` line per fill birth.
pub(crate) fn probe(m: &Model) -> Vec<String> {
    births(m)
        .into_iter()
        .map(|(mi, offset, k)| format!("# fill birth module={mi} offset={offset} len={k}"))
        .collect()
}
