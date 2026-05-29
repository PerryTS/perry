fn heap_value_type(value: f64) -> Option<(*const u8, u8)> {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let ptr = jv.as_pointer::<u8>();
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    unsafe {
        let gc_header = ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        Some((ptr, (*gc_header).obj_type))
    }
}

fn deep_strict_map_equal(
    left: *const crate::map::MapHeader,
    right: *const crate::map::MapHeader,
    depth: usize,
) -> bool {
    let len = crate::map::js_map_size(left);
    if len != crate::map::js_map_size(right) {
        return false;
    }
    let mut matched = vec![false; len as usize];
    for left_index in 0..len {
        let left_key = crate::map::js_map_entry_key_at(left, left_index);
        let left_value = crate::map::js_map_entry_value_at(left, left_index);
        let mut found = false;
        for right_index in 0..len {
            if matched[right_index as usize] {
                continue;
            }
            let right_key = crate::map::js_map_entry_key_at(right, right_index);
            if !super::js_util_deep_strict_equal_bool(left_key, right_key, depth + 1) {
                continue;
            }
            let right_value = crate::map::js_map_entry_value_at(right, right_index);
            if super::js_util_deep_strict_equal_bool(left_value, right_value, depth + 1) {
                matched[right_index as usize] = true;
                found = true;
                break;
            }
        }
        if !found {
            return false;
        }
    }
    true
}

fn deep_strict_set_equal(
    left: *const crate::set::SetHeader,
    right: *const crate::set::SetHeader,
    depth: usize,
) -> bool {
    let len = crate::set::js_set_size(left);
    if len != crate::set::js_set_size(right) {
        return false;
    }
    let mut matched = vec![false; len as usize];
    for left_index in 0..len {
        let left_value = crate::set::js_set_value_at(left, left_index);
        let mut found = false;
        for right_index in 0..len {
            if matched[right_index as usize] {
                continue;
            }
            let right_value = crate::set::js_set_value_at(right, right_index);
            if super::js_util_deep_strict_equal_bool(left_value, right_value, depth + 1) {
                matched[right_index as usize] = true;
                found = true;
                break;
            }
        }
        if !found {
            return false;
        }
    }
    true
}

pub(super) fn deep_strict_collection_equal(left: f64, right: f64, depth: usize) -> Option<bool> {
    let left_heap = heap_value_type(left);
    let right_heap = heap_value_type(right);
    match (left_heap, right_heap) {
        (Some((left_ptr, crate::gc::GC_TYPE_MAP)), Some((right_ptr, crate::gc::GC_TYPE_MAP))) => {
            Some(deep_strict_map_equal(
                left_ptr as *const crate::map::MapHeader,
                right_ptr as *const crate::map::MapHeader,
                depth,
            ))
        }
        (Some((left_ptr, crate::gc::GC_TYPE_SET)), Some((right_ptr, crate::gc::GC_TYPE_SET))) => {
            Some(deep_strict_set_equal(
                left_ptr as *const crate::set::SetHeader,
                right_ptr as *const crate::set::SetHeader,
                depth,
            ))
        }
        (Some((_, crate::gc::GC_TYPE_MAP | crate::gc::GC_TYPE_SET)), _)
        | (_, Some((_, crate::gc::GC_TYPE_MAP | crate::gc::GC_TYPE_SET))) => Some(false),
        _ => None,
    }
}
