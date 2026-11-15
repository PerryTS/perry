//! An element key the body's range proofs bound to the int32 range, of
//! either sign.
//!
//! The canonical key split (`index_get`'s runtime-key arm, `index_set`'s
//! runtime-key store) admits a key that is only statically numeric through
//! the shared normalizer (`index_key`, a call) unless the key's lowered
//! double has a native i32 source. A key whose range is proven inside int32
//! ([`proven`]: `i32_fast_path::is_known_i32_range`, so a loop fact, a typed
//! body's parameter interval, or the whole-program interval of an int32
//! `let`, `range_facts::facts_local_range`) is an exact int32 at every
//! evaluation, so it is lowered as one ([`lower`]: its i32 chain, else one
//! conversion of its double) and handed to the split as that source. The
//! split's unsigned bound (N10) then declines a negative key onto the arm
//! that reads the original value, as it does for any native key.
//!
//! `PERRY_FACTS_LOCAL_RANGES=0` keeps the double key and the normalizer.

use anyhow::Result;
use perry_hir::Expr;

use super::FnCtx;
use crate::types::{DOUBLE, I32};

/// `index` is an int32 at every evaluation by the body's range proofs, and
/// the unsigned bound is on (the split then needs no sign test for it).
pub(crate) fn proven(ctx: &FnCtx<'_>, index: &Expr) -> bool {
    super::facts_local_ranges()
        && super::index_key::unsigned_bound()
        && !ctx.wpfacts_off
        && super::i32_fast_path::is_known_i32_range(ctx, index)
}

/// The key `index` ([`proven`]) as the double whose source is its i32
/// (`sitofp`): `index_key::normalize_unsigned` hands that i32 over as it is.
pub(crate) fn lower(ctx: &mut FnCtx<'_>, index: &Expr) -> Result<String> {
    let idx = super::lower_expr_as_i32(ctx, index)?;
    Ok(ctx.block().sitofp(I32, &idx, DOUBLE))
}
