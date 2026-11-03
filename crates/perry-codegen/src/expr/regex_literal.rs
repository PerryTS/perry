//! A RegExp literal's evaluation: a fresh ordinary RegExp around the site's
//! immutable matcher data (`perry-runtime/src/regex/literal.rs`).
//!
//! The site is a static descriptor: source/flags string descriptors and matcher
//! data, registered as a global root on the first evaluation. Every evaluation is one
//! call to the shared entry `js_regexp_literal(site)`, whose
//! birth is the runtime's ordinary RegExp birth (`regex/instance.rs`); the
//! site carries no birth sequence and no birth state of its own.
use super::FnCtx;
use crate::types::I64;

/// The descriptor keeps operand loads on the shared cold miss, so every
/// evaluation emits only the descriptor address and the shared call.
fn emit_regexp_site(ctx: &mut FnCtx<'_>, pattern_global: &str, flags_global: &str) -> String {
    let site_id = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let prefix = ctx.strings.module_prefix();
    let slot_name = if prefix.is_empty() {
        format!("perry_regexp_site_{site_id}")
    } else {
        format!("perry_regexp_site_{prefix}__{site_id}")
    };
    ctx.typed_parse_rodata.push(format!(
        "@{slot_name} = private global {{ ptr, ptr, i64 }} {{ ptr {pattern_global}, ptr {flags_global}, i64 0 }}"
    ));
    format!("@{slot_name}")
}

/// Lower `/pattern/flags` to an i64 object handle.
pub(crate) fn lower_regexp_literal(ctx: &mut FnCtx<'_>, pattern: &str, flags: &str) -> String {
    let pattern_idx = ctx.strings.intern(pattern);
    let flags_idx = ctx.strings.intern(flags);
    // The site is static data: it names the immutable dispatch descriptors,
    // never a string handle (thread-local under agents, and a thread-local's
    // address is not a pointer to its value on Mach-O).
    let pattern_global = format!("@{}", ctx.strings.static_dispatch_global(pattern_idx));
    let flags_global = format!("@{}", ctx.strings.static_dispatch_global(flags_idx));
    let site = emit_regexp_site(ctx, &pattern_global, &flags_global);
    let blk = ctx.block();
    let site_key = blk.ptrtoint(&site, I64);
    blk.call(I64, "js_regexp_literal", &[(I64, &site_key)])
}
