//! The full-outline collapse of the instance method-dispatch towers (#5391
//! path 4): in an oversized module the class-id switch towers are not
//! emitted; the call takes the universal method site
//! (`crate::expr::method_site`), whose memo is keyed on the receiver's shape.

/// Whether to collapse the instance method-dispatch towers in full-outline
/// mode. On by default whenever full-outline is active;
/// `PERRY_OUTLINE_METHOD_DISPATCH=0` / `off` / `false` keeps the inline
/// class-id switch towers (escape hatch / differential-test isolation).
pub(super) fn method_dispatch_collapse_enabled() -> bool {
    !matches!(
        std::env::var("PERRY_OUTLINE_METHOD_DISPATCH").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}
