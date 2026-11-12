//! The verdict of a by-name method site (wpfacts `method_sites`).
//!
//! A method call whose receiver's STATIC type names a built-in kind is
//! lowered to that kind's body by name (`lower_array_method`, the folded
//! `Expr::Array*` nodes). The annotation is no proof of the value: behind
//! `a: Float64Array` there may be an ordinary object with its own `fill`, a
//! class instance, a subclass instance, `null`. The whole-program facts
//! judge each site by the values its receiver holds; this module is how
//! codegen asks, with the key both sides compute
//! (`perry_hir::folded_call::Site` and the method name).
//!
//! Typed arrays: a site whose receiver is annotated a typed array keeps its
//! typed-array lowering only when the facts prove the receiver holds typed
//! arrays the realm bore and nothing else
//! (`perry_hir::folded_call::SITE_TYPED_ARRAY`). Every other such site is
//! the ordinary method call: the real `Get` and `Call`, which runs an own or
//! inherited method of any receiver and throws for a missing member or a
//! `null` / `undefined` receiver. The fact says nothing about the member (an
//! own property or a prototype write on a real typed array is not seen by
//! it).
//!
//! `PERRY_FACTS_TYPED_ARRAY_SITES=0` trusts the annotation, as before.

use crate::expr::FnCtx;
use perry_hir::folded_call::{Site, SITE_TYPED_ARRAY};
use perry_hir::Expr;

/// `PERRY_FACTS_TYPED_ARRAY_SITES=0` (`off`, `false`) keeps the typed-array
/// lowering of every annotated receiver. Read per call; a compile-cache
/// input.
pub(crate) fn typed_array_sites_enabled() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_TYPED_ARRAY_SITES").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// Does the STATIC type of `object` select the typed-array flavour of the
/// by-name Array lowering? The ten element-typed kinds `is_array_expr`
/// admits; `Uint8Array` and `Uint8ClampedArray` are dispatched on the value
/// by the runtime and are not lowered from their annotation.
pub(crate) fn annotated_typed_array(ctx: &FnCtx<'_>, object: &Expr) -> bool {
    crate::type_analysis::is_typed_array_expr(ctx, object)
        && crate::type_analysis::is_array_expr(ctx, object)
}

/// Do the facts prove that the receiver of the method site `site` named
/// `name` holds only typed arrays? No site, no facts or no record: no.
pub(crate) fn typed_array_receiver_proven(
    ctx: &FnCtx<'_>,
    site: Option<Site>,
    name: &str,
) -> bool {
    let Some(site) = site else {
        return false;
    };
    !ctx.wpfacts_off
        && ctx
            .wpfacts
            .method_sites
            .get(&(site, name.to_string()))
            .is_some_and(|flags| flags & SITE_TYPED_ARRAY != 0)
}

/// Is the method call `object.name(..)` at `site` one whose receiver is
/// annotated a typed array and not proven to hold one? Such a call is the
/// ordinary method call.
///
/// A proven site keeps its lowering and, in check mode, tests its claim on
/// the receiver value first ([`witness_typed_array`]). The test needs the
/// value before the lowering evaluates it, so check mode consumes the fact
/// only for a receiver that is read again for free (a local, `this`); any
/// other receiver is the ordinary call there.
pub(crate) fn unproven_typed_array_call(
    ctx: &mut FnCtx<'_>,
    site: Option<Site>,
    object: &Expr,
    name: &str,
) -> anyhow::Result<bool> {
    if !typed_array_sites_enabled() || !annotated_typed_array(ctx, object) {
        return Ok(false);
    }
    let witnessed = !crate::verify_facts::enabled() || matches!(object, Expr::LocalGet(_) | Expr::This);
    let proven = witnessed && typed_array_receiver_proven(ctx, site, name);
    if std::env::var("PERRY_METHOD_SITE_DIAG").as_deref() == Ok("1") {
        eprintln!(
            "[perry method_site] fn={} kind=typed_array key={name} verdict={}",
            ctx.func.name,
            if proven { "proven" } else { "ordinary-call" }
        );
    }
    if proven {
        witness_typed_array(ctx, object, name)?;
    }
    Ok(!proven)
}

/// A site that keeps its typed-array lowering by the facts: the consumption
/// record, and in check mode the test that the receiver value is a typed
/// array ("method site").
fn witness_typed_array(ctx: &mut FnCtx<'_>, object: &Expr, name: &str) -> anyhow::Result<()> {
    use crate::types::{DOUBLE, I32, I64};
    crate::facts_consumed::used();
    crate::verify_facts::covered(crate::verify_facts::Fact::MethodSite, || {
        serde_json::json!({ "key": name, "receiver": "typed array" })
    });
    if !crate::verify_facts::enabled() {
        return Ok(());
    }
    let recv = crate::expr::lower_expr(ctx, object)?;
    let answer = ctx
        .block()
        .call(DOUBLE, "js_util_types_is_typed_array", &[(DOUBLE, &recv)]);
    let bits = ctx.block().bitcast_double_to_i64(&answer);
    let holds = ctx
        .block()
        .icmp_eq(I64, &bits, crate::nanbox::TAG_TRUE_I64);
    let miss = ctx.new_block("methodsite.verify_miss");
    let next = ctx.new_block("methodsite.admitted");
    let (miss_l, next_l) = (ctx.block_label(miss), ctx.block_label(next));
    crate::verify_facts::tick(ctx);
    ctx.block().cond_br(&holds, &next_l, &miss_l);
    ctx.current_block = miss;
    let what = format!("the receiver of .{name}() holds only typed arrays");
    let site = crate::verify_facts::site_arg(ctx, &what, crate::verify_facts::Fact::MethodSite, &[]);
    let _ = crate::verify_facts::check(
        ctx,
        I32,
        "perry_wpfacts_check_miss",
        &[(DOUBLE, &recv)],
        &site,
    );
    ctx.block().unreachable();
    ctx.current_block = next;
    Ok(())
}
