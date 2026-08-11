use super::WEAK_HOLDERS;

thread_local! {
    static FULL_WEAK_PROCESSING_WORK_UNITS: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
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
