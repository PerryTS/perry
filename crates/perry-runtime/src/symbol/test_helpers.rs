//! Owned symbol/class fixture helpers; no registry or scanner.
#[cfg(test)]
pub(crate) fn test_seed_symbol_property_root(owner: usize, sym_key: usize, value_bits: u64) {
    super::store_object_symbol_property_root(owner, sym_key, value_bits);
}

#[cfg(test)]
pub(crate) fn test_symbol_property_root_bits(owner: usize, sym_key: usize) -> Option<u64> {
    super::symbol_property_root_bits(owner, sym_key)
}

#[cfg(test)]
pub(crate) fn test_seed_class_static_symbol_root(class_id: u32, sym_key: usize, value_bits: u64) {
    super::store_class_static_symbol_root(class_id, sym_key, value_bits);
}

#[cfg(test)]
pub(crate) fn test_class_static_symbol_root_bits(class_id: u32, sym_key: usize) -> Option<u64> {
    let owner = crate::object::class_value::class_value_ptr(class_id) as usize;
    super::symbol_property_root_bits(owner, sym_key)
}

#[cfg(test)]
pub(crate) fn test_class_static_symbol_roots_for_class(class_id: u32) -> Vec<(usize, u64)> {
    let owner = crate::object::class_value::class_value_ptr(class_id) as usize;
    super::clone_symbol_entries_for_obj_ptr(owner)
}
