//! Symbol identity across native and emitted cells. Registry symbols are
//! identified by their registry key; other symbols carry their header id.
use super::*;

/// Borrow the registry key from a proven symbol, without materialization.
/// Native text and image strings both live as long as their symbol.
pub(crate) unsafe fn registry_key(symbol: &SymbolHeader) -> Option<&[u8]> {
    if symbol.registered & perry_abi::SYMBOL_GLOBAL_REGISTRY == 0 {
        return None;
    }
    if symbol.registered & perry_abi::SYMBOL_NATIVE_TEXT != 0 {
        let cell = (symbol as *const SymbolHeader as usize - crate::gc::GC_HEADER_SIZE)
            as *const PersistentSymbol;
        return (*cell)
            .description_text
            .as_ref()
            .map(|text| text.as_bytes());
    }
    let text = symbol.description;
    if text.is_null() {
        return None;
    }
    Some(std::slice::from_raw_parts(
        crate::string::string_data(text),
        (*text).byte_len as usize,
    ))
}

/// Hash of the same identity that `js_symbol_equals` compares.
/// The caller has proved `symbol` is a live SymbolHeader.
pub(crate) unsafe fn identity_hash(symbol: *const SymbolHeader) -> u64 {
    match registry_key(&*symbol) {
        Some(key) => crate::object::key_bytes_hash(key.as_ptr(), key.len()),
        None => (*symbol).id,
    }
}

/// Owned, move-stable identity for grouping. This is the key of the grouping
/// operation's existing index; it retains no symbol address or GC edge.
#[derive(Eq, Hash, PartialEq)]
pub(crate) enum Identity {
    Registry(Vec<u8>),
    Serial(u64),
}

pub(crate) unsafe fn identity(symbol: *const SymbolHeader) -> Identity {
    match registry_key(&*symbol) {
        Some(key) => Identity::Registry(key.to_vec()),
        None => Identity::Serial((*symbol).id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_identity_crosses_image_and_native_representations() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        unsafe {
            let key = "rb6.registry.cross.cell";
            let text = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
            let native = js_symbol_for(crate::value::js_nanbox_string(text as i64));
            // Image shape: registry-kind header, traced text, independent id.
            let image = alloc_symbol(text, true);
            (*image).id = perry_abi::symbol_identity(perry_abi::IMAGE_SYMBOL_ORDINAL_BASE + 1000);
            let image = crate::value::js_nanbox_pointer(image as i64);
            let fresh = js_symbol_new(crate::value::js_nanbox_string(text as i64));
            let a = crate::value::js_nanbox_get_pointer(native) as *const SymbolHeader;
            let b = crate::value::js_nanbox_get_pointer(image) as *const SymbolHeader;
            assert_ne!((*a).id, (*b).id);
            assert_eq!(js_symbol_equals(native, image), 1);
            assert_eq!(js_symbol_equals(native, fresh), 0);
            assert_eq!(identity_hash(a), identity_hash(b));
            assert!(identity(a) == identity(b));
            for fillers in [0, 40] {
                let map = crate::map::js_map_alloc(4);
                let set = crate::set::js_set_alloc(4);
                let obj = crate::object::js_object_alloc(0, 0);
                let value = crate::value::js_nanbox_pointer(obj as i64);
                for i in 0..fillers {
                    crate::map::js_map_set(map, i as f64, -1.0);
                    crate::set::js_set_add(set, i as f64);
                }
                if fillers != 0 {
                    crate::object::overflow::ensure(obj);
                }
                crate::map::js_map_set(map, image, 17.0);
                crate::set::js_set_add(set, image);
                js_object_set_symbol_property(value, image, 17.0);
                assert_eq!(crate::map::js_map_get(map, native), 17.0);
                assert_eq!(crate::set::js_set_has(set, native), 1);
                assert_eq!(js_object_get_symbol_property(value, native), 17.0);
                assert_eq!(crate::map::js_map_has(map, fresh), 0);
                assert_eq!(crate::set::js_set_has(set, fresh), 0);
                js_object_set_symbol_property(value, native, 23.0);
                assert_eq!(js_object_get_symbol_property(value, image), 23.0);
                assert_eq!(
                    crate::array::js_array_length(
                        js_object_get_own_property_symbols(value) as *const crate::ArrayHeader
                    ),
                    1
                );
            }
        }
    }
}
