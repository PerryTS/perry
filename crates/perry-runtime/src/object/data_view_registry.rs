use super::*;
use crate::fast_hash::{new_ptr_hash_set, PtrHashSet};
use crate::object::class_image::ImageTable;
use crate::object::class_meta_registry::{any_registered_ancestor, parent_edge_count_bound};

/// The calling image's set of class IDs that extend the built-in DataView
/// class (#8546 — see `object/class_image.rs`).
static EXTENDS_DATA_VIEW_REGISTRY: ImageTable<RwLock<Option<PtrHashSet<u32>>>> =
    ImageTable::new(|image| &image.extends_data_view);
static EXTENDS_TYPED_ARRAY_REGISTRY: ImageTable<RwLock<Option<PtrHashSet<u32>>>> =
    ImageTable::new(|image| &image.extends_typed_array);

/// Mark a user-defined class as extending the built-in DataView class.
#[no_mangle]
pub extern "C" fn js_register_class_extends_data_view(class_id: u32) {
    let mut registry = EXTENDS_DATA_VIEW_REGISTRY.write().unwrap();
    if registry.is_none() {
        *registry = Some(new_ptr_hash_set());
    }
    registry.as_mut().unwrap().insert(class_id);
}

/// Check if a class id extends the built-in DataView class.
pub(crate) fn extends_builtin_data_view(class_id: u32) -> bool {
    let registry = EXTENDS_DATA_VIEW_REGISTRY.read().unwrap();
    let Some(reg) = registry.as_ref() else {
        return false;
    };
    reg.contains(&class_id) || any_registered_ancestor(class_id, 32, |p| reg.contains(&p))
}

#[no_mangle]
pub extern "C" fn js_register_class_extends_typed_array(class_id: u32) {
    let mut registry = EXTENDS_TYPED_ARRAY_REGISTRY.write().unwrap();
    registry
        .get_or_insert_with(new_ptr_hash_set)
        .insert(class_id);
}

pub(crate) fn register_builtin_view_parent(class_id: u32, parent_name: &str) {
    if parent_name == "DataView" {
        js_register_class_extends_data_view(class_id);
    } else if crate::typedarray::kind_for_name(parent_name).is_some() {
        js_register_class_extends_typed_array(class_id);
    }
}

pub(crate) fn extends_builtin_typed_array(class_id: u32) -> bool {
    let registry = EXTENDS_TYPED_ARRAY_REGISTRY.read().unwrap();
    let Some(registered) = registry.as_ref() else {
        return false;
    };
    // A valid parent chain cannot visit more entries than the registry
    // contains. This follows arbitrarily deep user hierarchies while still
    // terminating if malformed registry data contains a cycle.
    registered.contains(&class_id)
        || any_registered_ancestor(class_id, parent_edge_count_bound(), |p| {
            registered.contains(&p)
        })
}
