use super::*;

/// Resolve expandos on registry-backed AsyncResource handles before ordinary
/// object/handle property dispatch. Copy the key payload so hook code cannot
/// invalidate a borrowed slice by triggering a moving collection.
pub(crate) fn async_resource_property(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
    if key.is_null() {
        return None;
    }
    let key = unsafe { crate::string::OwnedStringBytes::copy_from_header(key) };
    let name = std::str::from_utf8(key.as_bytes()).ok()?;
    crate::async_hooks::try_async_resource_property_dispatch(obj as i64, name)
        .map(|value| JSValue::from_bits(value.to_bits()))
}
