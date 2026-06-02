use super::*;

/// Global registry of class IDs that extend the built-in DataView class.
static EXTENDS_DATA_VIEW_REGISTRY: RwLock<Option<std::collections::HashSet<u32>>> =
    RwLock::new(None);

/// Mark a user-defined class as extending the built-in DataView class.
#[no_mangle]
pub extern "C" fn js_register_class_extends_data_view(class_id: u32) {
    let mut registry = EXTENDS_DATA_VIEW_REGISTRY.write().unwrap();
    if registry.is_none() {
        *registry = Some(std::collections::HashSet::new());
    }
    registry.as_mut().unwrap().insert(class_id);
}

/// Check if a class id extends the built-in DataView class.
pub(crate) fn extends_builtin_data_view(class_id: u32) -> bool {
    let registry = EXTENDS_DATA_VIEW_REGISTRY.read().unwrap();
    if let Some(reg) = registry.as_ref() {
        if reg.contains(&class_id) {
            return true;
        }
        let mut current = class_id;
        let parent_reg = super::CLASS_REGISTRY.read().unwrap();
        if let Some(pr) = parent_reg.as_ref() {
            for _ in 0..32 {
                match pr.get(&current).copied() {
                    Some(parent) if parent != 0 => {
                        if reg.contains(&parent) {
                            return true;
                        }
                        current = parent;
                    }
                    _ => break,
                }
            }
        }
    }
    false
}
