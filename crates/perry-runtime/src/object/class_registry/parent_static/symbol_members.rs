use super::*;

pub(crate) fn lookup_class_symbol_method_in_chain(
    class_id: u32,
    sym_key: usize,
    is_static: bool,
) -> Option<(usize, u32, bool)> {
    CLASS_SYMBOL_METHODS.with(|table| {
        let guard = table.read().ok()?;
        let map = guard.as_ref()?;
        let mut cid = class_id;
        let mut depth = 0usize;
        while cid != 0 && depth < 32 {
            if let Some(&entry) = map.get(&(cid, sym_key, is_static)) {
                return Some(entry);
            }
            match get_parent_class_id(cid) {
                Some(p) if p != 0 && p != cid => {
                    cid = p;
                    depth += 1;
                }
                _ => break,
            }
        }
        None
    })
}

/// Presence-only check (`[[HasProperty]]`, never `[[Get]]`) for a Symbol-keyed
/// METHOD or ACCESSOR declared on `class_id` or any ancestor. These computed
/// members register into `CLASS_SYMBOL_METHODS` / `CLASS_SYMBOL_ACCESSORS`, which
/// the generic symbol resolver (`js_object_get_symbol_property`) does NOT consult
/// — so `sym in Class` reported false even though `Class[sym](...)` dispatches
/// fine through the direct-call path. Walks the parent chain like
/// `lookup_class_symbol_method_in_chain`, but returns a bool and also covers
/// accessors so a static/instance `get [sym]()` is detected without invoking the
/// getter. Refs #6160.
pub(crate) fn class_has_symbol_member_in_chain(
    class_id: u32,
    sym_key: usize,
    is_static: bool,
) -> bool {
    let mut cid = class_id;
    let mut depth = 0usize;
    while cid != 0 && depth < 32 {
        let key = (cid, sym_key, is_static);
        let in_methods = CLASS_SYMBOL_METHODS.with(|table| {
            table
                .read()
                .ok()
                .and_then(|g| g.as_ref().map(|m| m.contains_key(&key)))
                .unwrap_or(false)
        });
        if in_methods {
            return true;
        }
        let in_accessors = CLASS_SYMBOL_ACCESSORS.with(|table| {
            table
                .read()
                .ok()
                .and_then(|g| g.as_ref().map(|m| m.contains_key(&key)))
                .unwrap_or(false)
        });
        if in_accessors {
            return true;
        }
        match get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    false
}

pub(crate) fn class_own_symbol_member_keys(class_id: u32, is_static: bool) -> Vec<usize> {
    let mut keys = Vec::new();
    CLASS_SYMBOL_METHODS.with(|table| {
        if let Ok(methods) = table.read() {
            if let Some(map) = methods.as_ref() {
                for &(cid, sym_key, static_flag) in map.keys() {
                    if cid == class_id && static_flag == is_static && !keys.contains(&sym_key) {
                        keys.push(sym_key);
                    }
                }
            }
        }
    });
    CLASS_SYMBOL_ACCESSORS.with(|table| {
        if let Ok(accessors) = table.read() {
            if let Some(map) = accessors.as_ref() {
                for &(cid, sym_key, static_flag) in map.keys() {
                    if cid == class_id && static_flag == is_static && !keys.contains(&sym_key) {
                        keys.push(sym_key);
                    }
                }
            }
        }
    });
    crate::cold_sort::sort_by_key(&mut keys, |sym_key| unsafe {
        let ptr = *sym_key as *const crate::symbol::SymbolHeader;
        let symbol_id = if ptr.is_null() { u64::MAX } else { (*ptr).id };
        let definition_order = CLASS_SYMBOL_MEMBER_ORDERS.with(|orders| {
            orders.read().ok().and_then(|guard| {
                guard
                    .as_ref()
                    .and_then(|map| map.get(&(class_id, symbol_id, is_static)).copied())
            })
        });
        if let Some(order) = definition_order {
            (0u8, order, symbol_id)
        } else {
            (1u8, u32::MAX, symbol_id)
        }
    });
    keys
}

pub(crate) unsafe fn class_symbol_getter_value(
    class_id: u32,
    sym_key: usize,
    receiver: f64,
    is_static: bool,
) -> Option<f64> {
    CLASS_SYMBOL_ACCESSORS.with(|table| {
        let guard = table.read().ok()?;
        let map = guard.as_ref()?;
        let mut cid = class_id;
        let mut depth = 0usize;
        while cid != 0 && depth < 32 {
            if let Some(&(getter, _)) = map.get(&(cid, sym_key, is_static)) {
                if getter == 0 {
                    return Some(f64::from_bits(crate::value::TAG_UNDEFINED));
                }
                let result = if is_static {
                    crate::object::static_private_owner_push(receiver);
                    let f = crate::closure::body_call::js_bare_body_fn!(getter as *const u8;);
                    let result = f();
                    crate::object::static_private_owner_pop();
                    result
                } else {
                    let f = crate::closure::body_call::js_method_body_fn!(getter as *const u8;);
                    f(receiver)
                };
                return Some(result);
            }
            match get_parent_class_id(cid) {
                Some(p) if p != 0 && p != cid => {
                    cid = p;
                    depth += 1;
                }
                _ => break,
            }
        }
        None
    })
}

pub(crate) unsafe fn class_symbol_setter_apply(
    class_id: u32,
    sym_key: usize,
    receiver: f64,
    value: f64,
    is_static: bool,
) -> bool {
    CLASS_SYMBOL_ACCESSORS.with(|table| {
        let guard = match table.read() {
            Ok(g) => g,
            Err(_) => return false,
        };
        let Some(map) = guard.as_ref() else {
            return false;
        };
        let mut cid = class_id;
        let mut depth = 0usize;
        while cid != 0 && depth < 32 {
            if let Some(&(_, setter)) = map.get(&(cid, sym_key, is_static)) {
                if setter != 0 {
                    if is_static {
                        crate::object::static_private_owner_push(receiver);
                        let f =
                            crate::closure::body_call::js_bare_body_fn!(setter as *const u8; a0);
                        let _ = f(value);
                        crate::object::static_private_owner_pop();
                    } else {
                        let f =
                            crate::closure::body_call::js_method_body_fn!(setter as *const u8; a0);
                        let _ = f(receiver, value);
                    }
                }
                return true;
            }
            match get_parent_class_id(cid) {
                Some(p) if p != 0 && p != cid => {
                    cid = p;
                    depth += 1;
                }
                _ => break,
            }
        }
        false
    })
}
