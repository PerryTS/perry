//! Descriptor bags on existing native stream records, separate from their
//! typed interfaces. No per-stream property address table.
use super::state::{clear_props_in_freed_ranges, with_props};
pub(crate) fn native_property_bag(id: usize) -> Option<u64> {
    with_props(id, |props| props.get()).flatten()
}
pub(crate) fn set_native_property_bag(id: usize, bits: u64) -> bool {
    perry_runtime::arena::thread_exit::register_thread_exit_range_hook(
        clear_native_bags_in_freed_ranges,
    );
    with_props(id, |props| props.set(bits)).unwrap_or(false)
}
fn clear_native_bags_in_freed_ranges(freed: &perry_runtime::arena::thread_exit::FreedRanges) {
    clear_props_in_freed_ranges(freed);
}
