//! Resolve const TLS storage before running page-metadata operations.

use super::{PageGenerationMap, PAGE_GENERATIONS};
use std::cell::RefCell;

/// Address of this thread's page-generation map, also used by the hot cache.
/// Keep the TLS callback small: putting the entire registration loop inside
/// LocalKey::with can retain an opaque TLS-getter call in production builds.
#[inline(never)]
pub(crate) fn page_generations_hot_addr() -> *mut u8 {
    PAGE_GENERATIONS.with(|pages| std::ptr::from_ref(pages).cast_mut().cast())
}

/// Scoped access through the same address resolver as the hot cache.
#[inline(always)]
pub(super) fn with_page_generations<R>(f: impl FnOnce(&RefCell<PageGenerationMap>) -> R) -> R {
    let pages = page_generations_hot_addr().cast::<RefCell<PageGenerationMap>>();
    // The standard LocalKey access above rejects access after destruction.
    // Its value cannot be destroyed until this thread exits, after this
    // synchronous callback has returned; no reference escapes this scope.
    unsafe { f(&*pages) }
}
