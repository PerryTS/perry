//! The existing Fetch native records own their ordinary property bag.
use super::*;

pub(crate) fn native_property_bag(id: usize) -> Option<u64> {
    if !lifecycle::owns(id) {
        return None;
    }
    macro_rules! read {
        ($table:expr) => {
            if let Some(record) = $table.lock().ok()?.get(&id) {
                return Some(record.props);
            }
        };
    }
    read!(FETCH_RESPONSES);
    read!(HEADERS_REGISTRY);
    read!(REQUEST_REGISTRY);
    read!(BLOB_REGISTRY);
    read!(body_metadata::FORM_DATA_REGISTRY);
    None
}
pub(crate) fn set_native_property_bag(id: usize, bits: u64) -> bool {
    if !lifecycle::owns(id) {
        return false;
    }
    macro_rules! write {
        ($table:expr) => {
            if let Ok(mut table) = $table.lock() {
                if let Some(record) = table.get_mut(&id) {
                    record.props = bits;
                    return true;
                }
            }
        };
    }
    write!(FETCH_RESPONSES);
    write!(HEADERS_REGISTRY);
    write!(REQUEST_REGISTRY);
    write!(BLOB_REGISTRY);
    write!(body_metadata::FORM_DATA_REGISTRY);
    false
}
