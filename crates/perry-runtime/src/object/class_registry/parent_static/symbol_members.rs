use super::*;

pub(crate) fn lookup_class_symbol_method_in_chain(
    class_id: u32,
    sym_key: usize,
    is_static: bool,
) -> Option<(usize, u32, bool, usize)> {
    let mut cid = class_id;
    let mut depth = 0usize;
    while cid != 0 && depth < 32 {
        if let Some(entry) = class_own_symbol_method_entry(cid, sym_key, is_static) {
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
}

/// Presence-only check (`[[HasProperty]]`, never `[[Get]]`) for a Symbol-keyed
/// METHOD or ACCESSOR declared on `class_id` or any ancestor: a known-symbol
/// member by its declaration, an evaluated one by its registration. Refs #6160.
pub(crate) fn class_has_symbol_member_in_chain(
    class_id: u32,
    sym_key: usize,
    is_static: bool,
) -> bool {
    let mut cid = class_id;
    let mut depth = 0usize;
    while cid != 0 && depth < 32 {
        if class_has_own_symbol_member(cid, sym_key, is_static) {
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

/// The declared accessor halves of the first class on `class_id`'s chain
/// with an own instance symbol accessor `sym_key`, when a declaration (not a
/// registration) supplies it.
fn declared_symbol_accessor_in_chain(class_id: u32, sym_key: usize) -> Option<(usize, usize)> {
    let mut cid = class_id;
    let mut depth = 0usize;
    while cid != 0 && depth < 32 {
        if let Some(ptrs) = crate::object::class_registry::declarations::class_decl_symbol_accessor(cid, sym_key) {
            return Some(ptrs);
        }
        if class_own_symbol_accessor_ptrs(cid, sym_key, false).is_some() {
            return None;
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
}

pub(crate) fn class_own_symbol_member_keys(class_id: u32, is_static: bool) -> Vec<usize> {
    let mut keys = class_table_symbol_member_keys(class_id, is_static);
    if is_static {
        return keys;
    }
    let declared = crate::object::class_registry::declarations::class_declared_symbol_members(class_id);
    if declared.is_empty() {
        return keys;
    }
    let order = |sym: usize| {
        declared.iter().find(|e| e.sym == sym).and_then(|e| {
            let decl = crate::object::class_registry::declarations::class_declaration(class_id)?;
            let first = decl
                .members()
                .iter()
                .find(|m| m.symbol_key() == Some(e.sym))?;
            Some(first.definition_order)
        })
    };
    for e in &declared {
        if !keys.contains(&e.sym) {
            keys.push(e.sym);
        }
    }
    // Declared and registered members interleave by definition order.
    crate::cold_sort::sort_by_key(&mut keys, |&sym| {
        let recorded = unsafe {
            let ptr = sym as *const crate::symbol::SymbolHeader;
            let symbol_id = if ptr.is_null() { u64::MAX } else { (*ptr).id };
            CLASS_SYMBOL_MEMBER_ORDERS.with(|orders| {
                orders.read().ok().and_then(|guard| {
                    guard
                        .as_ref()
                        .and_then(|map| map.get(&(class_id, symbol_id, false)).copied())
                })
            })
        };
        order(sym).or(recorded).unwrap_or(u32::MAX)
    });
    keys
}

/// The symbol keys of class `class_id`'s members its definition registered
/// (evaluated keys), in definition order.
pub(crate) fn class_table_symbol_member_keys(class_id: u32, is_static: bool) -> Vec<usize> {
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
    if !is_static {
        if let Some((getter, _)) = declared_symbol_accessor_in_chain(class_id, sym_key) {
            if getter == 0 {
                return Some(f64::from_bits(crate::value::TAG_UNDEFINED));
            }
            let f = crate::closure::body_call::js_method_body_fn!(getter as *const u8;);
            return Some(f(receiver));
        }
    }
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
                    // OrdinaryGet: the holder `cid` (an ancestor of `class_id`
                    // when inherited) only locates the accessor; the getter
                    // runs with `this === receiver`, the class the read started
                    // from. A static body binds `this` in its own prologue
                    // (`js_static_this_resolve_class`: the armed override, else
                    // its lexical class), so without the arm an inherited
                    // `Deep[Symbol.species]` answered `Own`. Same convention as
                    // `class_static_accessor_call_get` for a string key.
                    crate::object::static_this_arm_if_unarmed(receiver);
                    crate::object::static_private_owner_push(receiver);
                    let f = crate::closure::body_call::js_bare_body_fn!(getter as *const u8;);
                    let result = f();
                    crate::object::static_private_owner_pop();
                    crate::object::static_this_disarm();
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
    if !is_static {
        if let Some((_, setter)) = declared_symbol_accessor_in_chain(class_id, sym_key) {
            if setter != 0 {
                let f = crate::closure::body_call::js_method_body_fn!(setter as *const u8; a0);
                let _ = f(receiver, value);
            }
            return true;
        }
    }
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
                        // OrdinarySet: the setter runs with `this === receiver`
                        // (see the getter above); the holder only locates it.
                        crate::object::static_this_arm_if_unarmed(receiver);
                        crate::object::static_private_owner_push(receiver);
                        let f =
                            crate::closure::body_call::js_bare_body_fn!(setter as *const u8; a0);
                        let _ = f(value);
                        crate::object::static_private_owner_pop();
                        crate::object::static_this_disarm();
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

#[cfg(test)]
mod static_receiver_tests {
    use super::*;
    use std::cell::Cell;

    thread_local! {
        static SETTER_CALL: Cell<Option<(u64, u64)>> = const { Cell::new(None) };
    }

    /// A compiled static getter body: no receiver parameter; `this` is bound
    /// by the static prologue (`js_static_this_resolve`: the armed override,
    /// else the lexical class, which `undefined` stands in for here).
    extern "C" fn static_this_getter() -> f64 {
        crate::object::js_static_this_resolve(f64::from_bits(crate::value::TAG_UNDEFINED))
    }

    /// A compiled static setter body: records `(this, value)`.
    extern "C" fn static_recording_setter(value: f64) -> f64 {
        let this =
            crate::object::js_static_this_resolve(f64::from_bits(crate::value::TAG_UNDEFINED));
        SETTER_CALL.with(|c| c.set(Some((this.to_bits(), value.to_bits()))));
        f64::from_bits(crate::value::TAG_UNDEFINED)
    }

    /// OrdinaryGet/OrdinarySet: a static symbol accessor found on an ancestor
    /// runs with `this === receiver`, the class the read or write started
    /// from, not the holder (`Deep[Symbol.species]` answers `Deep`).
    #[test]
    fn inherited_static_symbol_accessor_runs_with_the_receiver() {
        unsafe {
            crate::object::class_registry::test_clear_class_side_table_roots();
            let own = 0x3a5c_u32;
            let deep = 0x3a5d_u32;
            let sym = crate::symbol::js_symbol_new_empty();
            let sym_key = crate::symbol::sym_key_from_f64(sym);
            crate::object::class_registry::js_register_class_computed_accessor(
                own as i64,
                sym,
                static_this_getter as *const () as usize as i64,
                static_recording_setter as *const () as usize as i64,
                1,
                0,
                f64::from_bits(crate::value::TAG_UNDEFINED),
            );
            super::super::register_class(deep, own);

            // Any value stands for the class `Deep` the read started from.
            let receiver = 42.0_f64;
            let got = class_symbol_getter_value(deep, sym_key, receiver, true)
                .expect("the accessor is found on the parent");
            assert_eq!(
                got.to_bits(),
                receiver.to_bits(),
                "an inherited static getter runs with the receiver, not the holder"
            );
            let own_receiver = 43.0_f64;
            let got_own = class_symbol_getter_value(own, sym_key, own_receiver, true)
                .expect("the accessor is the class's own");
            assert_eq!(got_own.to_bits(), own_receiver.to_bits());

            SETTER_CALL.with(|c| c.set(None));
            assert!(class_symbol_setter_apply(
                deep, sym_key, receiver, 7.0, true
            ));
            assert_eq!(
                SETTER_CALL.with(|c| c.get()),
                Some((receiver.to_bits(), 7.0_f64.to_bits())),
                "an inherited static setter runs with the receiver and the value"
            );
            // The one-shot override is consumed by the body's prologue; the
            // disarm after the call leaves nothing armed for the next body.
            assert_eq!(
                crate::object::js_static_this_resolve(1.0).to_bits(),
                1.0_f64.to_bits()
            );
            crate::object::class_registry::test_clear_class_side_table_roots();
        }
    }

    /// `(C as any)[S] = v` (`js_put_value_set_dynamic`, the [[Set]] walk): a
    /// declared `static set [S]` is an own accessor of the class function
    /// object, so an own or inherited setter runs with `this` = the class
    /// written to and stores nothing; a getter-only one refuses the write
    /// (no shadowing data member); a class without one takes a data member.
    #[test]
    fn class_symbol_write_runs_the_declared_static_setter() {
        unsafe {
            crate::object::class_registry::test_clear_class_side_table_roots();
            let own = 0x3a6c_u32;
            let deep = 0x3a6d_u32;
            let g = 0x3a6e_u32;
            let g2 = 0x3a6f_u32;
            let plain = 0x3a70_u32;
            for class_id in [own, deep, g, g2, plain] {
                crate::object::js_register_class_id(class_id);
            }
            super::super::register_class(own, 0);
            super::super::register_class(deep, own);
            super::super::register_class(g, 0);
            super::super::register_class(g2, g);
            super::super::register_class(plain, 0);
            let sym = crate::symbol::js_symbol_new_empty();
            let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
            crate::object::class_registry::js_register_class_computed_accessor(
                own as i64,
                sym,
                0,
                static_recording_setter as *const () as usize as i64,
                1,
                0,
                undef,
            );
            crate::object::class_registry::js_register_class_computed_accessor(
                g as i64,
                sym,
                static_this_getter as *const () as usize as i64,
                0,
                1,
                0,
                undef,
            );
            let set = |class_id: u32, v: f64| {
                let c = crate::object::class_value::class_value(class_id);
                crate::object::property_ops::put_value::js_put_value_set_dynamic(c, sym, v, 0);
                c
            };
            // An own symbol property of the class function object (an
            // accessor's entry or a data member).
            let own_symbol_data = |class_id: u32| {
                let c = crate::object::class_value::class_value(class_id);
                crate::symbol::symbol_property_root_bits(
                    crate::symbol::obj_key_from_f64(c),
                    crate::symbol::sym_key_from_f64(sym),
                )
            };
            let has_own = |class_id: u32| own_symbol_data(class_id).is_some();
            assert_eq!(
                crate::object::js_object_get_prototype_of(crate::object::class_value::class_value(
                    deep
                ))
                .to_bits(),
                crate::object::class_value::class_value(own).to_bits(),
                "the subclass function object inherits from its parent"
            );
            for (class_id, v) in [(own, 1.0_f64), (deep, 7.0)] {
                SETTER_CALL.with(|c| c.set(None));
                let c = set(class_id, v);
                assert_eq!(
                    SETTER_CALL.with(|c| c.get()),
                    Some((c.to_bits(), v.to_bits())),
                    "the static setter runs with this = the class written to (class {class_id:#x})"
                );
                let still_accessor = crate::symbol::symbol_accessor_descriptor_bits(
                    crate::symbol::obj_key_from_f64(c),
                    crate::symbol::sym_key_from_f64(sym),
                )
                .is_some();
                assert!(
                    still_accessor || own_symbol_data(class_id).is_none(),
                    "no data member shadows the setter (class {class_id:#x})"
                );
            }
            assert!(
                has_own(own),
                "the declared accessor is an own property of its class"
            );
            assert!(!has_own(deep), "an inherited accessor is not own");
            set(g2, 9.0);
            assert!(
                !has_own(g2),
                "a getter-only accessor refuses the write: nothing is stored"
            );
            set(plain, 3.0);
            assert_eq!(
                own_symbol_data(plain),
                Some(3.0_f64.to_bits()),
                "a class without a symbol accessor takes an own data member"
            );
            crate::object::class_registry::test_clear_class_side_table_roots();
        }
    }
}
