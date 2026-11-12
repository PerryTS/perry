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
pub(crate) fn unproven_typed_array_call(
    ctx: &FnCtx<'_>,
    site: Option<Site>,
    object: &Expr,
    name: &str,
) -> bool {
    if !typed_array_sites_enabled() || !annotated_typed_array(ctx, object) {
        return false;
    }
    let proven = typed_array_receiver_proven(ctx, site, name);
    if std::env::var_os("PERRY_METHOD_SITE_DIAG").is_some() {
        eprintln!(
            "[perry method_site] kind=typed_array key={name} site={site:?} verdict={}",
            if proven { "proven" } else { "ordinary-call" }
        );
    }
    !proven
}
