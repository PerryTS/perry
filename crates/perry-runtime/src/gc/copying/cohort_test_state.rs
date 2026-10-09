//! Test-only observation of whole-space and fresh-cohort copying counts.

thread_local! {
    /// Test-only witness for the #9851 follow-up: the whole-space pair against the
    /// fresh-cohort pair, as the copier computed them for one cycle. Without this
    /// the change is unfalsifiable from a test — the two quantities are equal on
    /// every heap whose survivor space holds a single generation, which is every
    /// heap at a threshold of 2 or below.
    static LAST_COHORT_SPLIT: std::cell::Cell<(usize, usize, usize, usize)> =
        const { std::cell::Cell::new((0, 0, 0, 0)) };
}

pub(super) fn test_record_cohort_split(
    copied_bytes: usize,
    eden_copied_bytes: usize,
    survivor_live_bytes: usize,
    first_round_live_bytes: usize,
) {
    LAST_COHORT_SPLIT.with(|c| {
        c.set((
            copied_bytes,
            eden_copied_bytes,
            survivor_live_bytes,
            first_round_live_bytes,
        ))
    });
}

/// `(copied_bytes, eden_copied_bytes, survivor_live_bytes, first_round_live_bytes)`
/// from the most recent copying minor on this thread.
pub(in crate::gc) fn test_last_cohort_split() -> (usize, usize, usize, usize) {
    LAST_COHORT_SPLIT.with(std::cell::Cell::get)
}
