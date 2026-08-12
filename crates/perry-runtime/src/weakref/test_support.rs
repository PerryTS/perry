use super::WEAK_HOLDERS;

thread_local! {
    static FULL_WEAK_PROCESSING_WORK_UNITS: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
    /// #7900: how many white objects the weak-READ barrier actually shaded.
    /// Tests assert this is non-zero so a green run cannot mean "the read
    /// happened to return an already-marked target" (CLAUDE.md: a gate must
    /// assert its subject was live).
    static WEAK_READ_BARRIER_SHADES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(crate) fn weak_read_barrier_shades() -> usize {
    WEAK_READ_BARRIER_SHADES.with(std::cell::Cell::get)
}

pub(crate) fn reset_weak_read_barrier_shades() {
    WEAK_READ_BARRIER_SHADES.with(|shades| shades.set(0));
}

pub(crate) fn note_weak_read_barrier_shade() {
    WEAK_READ_BARRIER_SHADES.with(|shades| shades.set(shades.get().saturating_add(1)));
}

pub(crate) fn full_weak_processing_work_units() -> usize {
    FULL_WEAK_PROCESSING_WORK_UNITS.with(std::cell::Cell::get)
}

pub(crate) fn reset_full_weak_processing_work_units() {
    FULL_WEAK_PROCESSING_WORK_UNITS.with(|units| units.set(0));
}

pub(crate) fn note_full_weak_processing_work_unit() {
    FULL_WEAK_PROCESSING_WORK_UNITS.with(|units| units.set(units.get().saturating_add(1)));
}

pub(crate) fn clear_weak_holders() {
    WEAK_HOLDERS.with(|holders| holders.borrow_mut().clear());
}

pub(crate) fn weak_holder_addresses() -> Vec<usize> {
    let mut addresses =
        WEAK_HOLDERS.with(|holders| holders.borrow().iter().copied().collect::<Vec<_>>());
    addresses.sort_unstable();
    addresses
}

pub(crate) fn register_weak_holder_address(addr: usize) {
    WEAK_HOLDERS.with(|holders| {
        holders.borrow_mut().insert(addr);
    });
}
