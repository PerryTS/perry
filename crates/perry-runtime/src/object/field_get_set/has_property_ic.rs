//! Presence sites use the existing keyed holder record, including its roots.
use crate::object::method_site::read_holder::keyed::{presence_at_site, KeyedCache};

#[no_mangle]
pub unsafe extern "C" fn js_in_operator_presence_ic(
    obj: f64,
    key: f64,
    slot: *mut *mut KeyedCache,
) -> f64 {
    presence_at_site(obj, key, slot)
}
