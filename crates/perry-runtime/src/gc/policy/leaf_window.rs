//! Leaf-service trigger arithmetic: when the leaf window is due ahead of the copying nursery cap.

#[cold]
pub(super) fn leaf_memory_due_at_cap(cap: usize, small_bytes: usize) -> (bool, usize) {
    // The caller's occupancy reading already excludes the leaf-free case;
    // leaf_window_due also answers zero leaves through its batching floor.
    let leaf_bytes = crate::arena::young_large_in_use_bytes();
    leaf_window_due(
        cap,
        small_bytes,
        crate::arena::copying_active_survivor_in_use_bytes(),
        leaf_bytes,
    )
}

/// Whether the young collection due at a safepoint is the leaf-only service
/// (census the young small cells, retire unnamed young leaves) rather than
/// the copying minor.
///
/// Only below the copying cap (`small_bytes < cap`), and only while the
/// allocation window the service reopens is at least the Eden it censuses,
/// `window - eden >= eden`. The census reads every young small cell, so its
/// cost grows with Eden, and it frees leaves, not Eden: it only defers the
/// copying minor by `window - eden`. With less than Eden's worth of window
/// left, the census reads more cells than the allocation it defers, so the
/// copying minor runs at the leaf arm's due point instead and retires the
/// dead leaves itself. `small_bytes` is the from-space
/// reading `leaf_window_due` takes; the survivor space is read only below the
/// cap, where the young arm can have been due through its leaf arm (at or
/// above it the copying cap is due and nothing else is read). The window is
/// the copying minor's own budget.
pub(in crate::gc) fn leaf_service_precedes_copying_cap(cap: usize, small_bytes: usize) -> bool {
    if small_bytes >= cap {
        return false;
    }
    let survivor_bytes = crate::arena::copying_active_survivor_in_use_bytes();
    let window = super::tenuring::eden_allocation_window_bytes(cap, survivor_bytes);
    let eden = small_bytes.saturating_sub(survivor_bytes);
    window.saturating_sub(eden) >= eden
}

/// The leaf arm of the young trigger: Eden plus the young leaves against
/// Eden's allocation window, and the from-space bound under which that stays
/// false. `small_bytes` is the from-space occupancy (Eden plus the active
/// survivor space); the window already credits the survivor space against the
/// cap (`eden_allocation_window_bytes`, the budget the copying minor reads),
/// so the survivor bytes come out of the reading before the comparison. Leaves
/// must also fill a quarter of that window to amortize the small-cell census;
/// below that floor only the unchanged small-copying cap can make young due.
/// Counting them on both sides made the arm due up to one survivor credit
/// before the copying cap with no leaf to retire, and every young leaf born
/// in that band ran a leaf service that could not make the arm false.
pub(in crate::gc) fn leaf_window_due(
    cap: usize,
    small_bytes: usize,
    survivor_bytes: usize,
    leaf_bytes: usize,
) -> (bool, usize) {
    let window = super::tenuring::eden_allocation_window_bytes(cap, survivor_bytes);
    // A leaf service censuses all young small cells. Near a full Eden, pricing
    // even one tiny leaf against the remaining window repeats that census for
    // almost no recovery, including at correctly recovered owed checks. Batch
    // leaves until they occupy a quarter-window; small occupancy still reaches
    // the unchanged copying cap and forces a tracing minor there.
    //
    // When this arm is quiet, either Eden + leaves is below the window, or
    // leaves are below a quarter-window. With small < cap the quiet bound is
    // max(survivor + window, cap + ceil(window / 4)): the first term is the
    // existing survivor-credit bound, the second is the new batching slack.
    // Quiet occupancy is therefore below 2 * cap for survivor <= small < cap,
    // before the existing allocation-point/safepoint slack. No prior service
    // decision is retained.
    if leaf_bytes < window.div_ceil(4) {
        return (false, usize::MAX);
    }
    let eden = small_bytes.saturating_sub(survivor_bytes);
    (
        eden.saturating_add(leaf_bytes) >= window,
        window
            .saturating_add(survivor_bytes)
            .saturating_sub(leaf_bytes),
    )
}
