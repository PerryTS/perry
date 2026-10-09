//! A RegExp literal's evaluation: a fresh ordinary RegExp around the site's
//! immutable matcher data (`perry-runtime/src/regex/literal.rs`).
//!
//! The site is two words. Word 0 holds the matcher data, a registered global
//! root the runtime publishes on the site's first evaluation. Word 1 holds
//! the instance's object-header word (`class_id | birth ShapeId << 32`) the
//! runtime publishes after it. Every evaluation is one call to the shared
//! entry `js_regexp_literal(pattern, flags, site)`, whose common path bumps
//! the inline arena and stamps word 1 (the runtime's `inline_birth`); the
//! site carries no birth sequence of its own.
use super::FnCtx;
use crate::types::{DOUBLE, I64};

/// The site's two words, `[2 x i64]`, unique per literal and zero until the
/// runtime publishes them.
pub(crate) fn emit_regexp_site(ctx: &mut FnCtx<'_>) -> String {
    let site_id = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let prefix = ctx.strings.module_prefix();
    let slot_name = if prefix.is_empty() {
        format!("perry_regexp_site_{site_id}")
    } else {
        format!("perry_regexp_site_{prefix}__{site_id}")
    };
    ctx.typed_parse_rodata.push(format!(
        "@{slot_name} = private global [2 x i64] zeroinitializer, align 8"
    ));
    format!("@{slot_name}")
}

/// Lower `/pattern/flags` to an i64 object handle.
pub(crate) fn lower_regexp_literal(ctx: &mut FnCtx<'_>, pattern: &str, flags: &str) -> String {
    let pattern_idx = ctx.strings.intern(pattern);
    let flags_idx = ctx.strings.intern(flags);
    let pattern_global = format!("@{}", ctx.strings.entry(pattern_idx).handle_global);
    let flags_global = format!("@{}", ctx.strings.entry(flags_idx).handle_global);
    let site = emit_regexp_site(ctx);
    let blk = ctx.block();
    let pattern_box = blk.load(DOUBLE, &pattern_global);
    let flags_box = blk.load(DOUBLE, &flags_global);
    let pattern_handle = super::unbox_to_i64(blk, &pattern_box);
    let flags_handle = super::unbox_to_i64(blk, &flags_box);
    let site_key = blk.ptrtoint(&site, I64);
    blk.call(
        I64,
        "js_regexp_literal",
        &[
            (I64, &pattern_handle),
            (I64, &flags_handle),
            (I64, &site_key),
        ],
    )
}
