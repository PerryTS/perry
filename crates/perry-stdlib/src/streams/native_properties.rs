//! Descriptor bags on existing native stream records, separate from their
//! typed interfaces. No per-stream property address table.
use super::*;
pub(crate) fn native_property_bag(id: usize) -> Option<u64> {
    macro_rules! read {
        ($table:expr) => {
            if let Some(record) = $table.lock().ok()?.get(&id) {
                return record.props.get();
            }
        };
    }
    read!(READABLE_STREAMS);
    read!(WRITABLE_STREAMS);
    read!(TRANSFORM_STREAMS);
    read!(READERS);
    read!(WRITERS);
    None
}
pub(crate) fn set_native_property_bag(id: usize, bits: u64) -> bool {
    macro_rules! write {
        ($table:expr) => {
            if let Ok(mut table) = $table.lock() {
                if let Some(record) = table.get_mut(&id) {
                    return record.props.set(bits);
                }
            }
        };
    }
    perry_runtime::arena::thread_exit::register_thread_exit_range_hook(
        clear_native_bags_in_freed_ranges,
    );
    write!(READABLE_STREAMS);
    write!(WRITABLE_STREAMS);
    write!(TRANSFORM_STREAMS);
    write!(READERS);
    write!(WRITERS);
    false
}
fn clear_native_bags_in_freed_ranges(freed: &perry_runtime::arena::thread_exit::FreedRanges) {
    macro_rules! clear {
        ($table:expr) => {
            if let Ok(mut table) = $table.lock() {
                for record in table.values_mut() {
                    record.props.clear_if(|address| freed.contains(address));
                }
            }
        };
    }
    clear!(READABLE_STREAMS);
    clear!(WRITABLE_STREAMS);
    clear!(TRANSFORM_STREAMS);
    clear!(READERS);
    clear!(WRITERS);
}
