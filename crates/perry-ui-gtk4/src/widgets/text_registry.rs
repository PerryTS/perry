use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static TEXT_REGISTRY: RefCell<HashMap<String, i64>> = RefCell::new(HashMap::new());
}

/// Store widget handle under id so setText() can find it later.
pub fn register(id: &str, handle: i64) {
    TEXT_REGISTRY.with(|r| {
        r.borrow_mut().insert(id.to_string(), handle);
    });
}

/// Update the GtkLabel text for a previously registered id.
pub fn set_text_for_id(id: &str, value: &str) {
    let handle = TEXT_REGISTRY.with(|r| r.borrow().get(id).copied());
    if let Some(h) = handle {
        super::text::set_text_str(h, value);
    }
}
