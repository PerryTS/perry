//! Named collection points for rooting regression tests.
//!
//! A runtime helper that holds a GC value in a Rust local across an allocation
//! only goes wrong when a moving collection lands in that exact window, which
//! an allocation-trigger test cannot aim at a specific allocation inside one
//! call. A test arms a site by name; the next time the helper passes that site
//! it runs a copying minor, so the test observes what the helper holds after a
//! collection at precisely that point. The same one-shot shape as
//! `set.rs`'s `test_force_next_set_helper_gc`, shared instead of repeated.
//!
//! Outside `cfg(test)` a collection point is an empty inline function.

#[cfg(test)]
crate::perry_thread_local! {
    static ARMED_SITE: std::cell::Cell<Option<(&'static str, u32)>> = const { std::cell::Cell::new(None) };
}

/// Run one copying minor the next time `collection_point(site)` is reached on
/// this thread.
#[cfg(test)]
pub(crate) fn arm_collection_point(site: &'static str) {
    arm_collection_point_after(site, 0);
}

/// Like [`arm_collection_point`], but skips the first `skip` hits on `site`
/// before firing on hit number `skip + 1`. A helper called once per loop
/// iteration only exposes ONE named site, so this is how a test puts the
/// forced collection on a LATER iteration than the first -- e.g. to check
/// that a value already read (and copied into a plain, unrooted local) on an
/// earlier iteration survives a collection triggered by a later one.
#[cfg(test)]
pub(crate) fn arm_collection_point_after(site: &'static str, skip: u32) {
    ARMED_SITE.with(|armed| armed.set(Some((site, skip))));
}

#[cfg(test)]
pub(crate) fn collection_point(site: &'static str) {
    let hit = ARMED_SITE.with(|armed| match armed.get() {
        Some((armed_site, 0)) if armed_site == site => {
            armed.set(None);
            true
        }
        Some((armed_site, remaining)) if armed_site == site => {
            armed.set(Some((armed_site, remaining - 1)));
            false
        }
        _ => false,
    });
    if hit {
        let _ = super::gc_collect_minor();
    }
}

#[cfg(not(test))]
#[inline(always)]
pub(crate) fn collection_point(_site: &'static str) {}
