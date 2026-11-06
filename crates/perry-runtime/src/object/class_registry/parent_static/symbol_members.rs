use super::*;

/// ClassBody symbol members are ordinary properties of their actual holder.
pub(crate) fn class_has_own_symbol_member(class_id: u32, sym_key: usize, is_static: bool) -> bool {
    let owner = if is_static {
        crate::object::class_value::class_value_if_minted(class_id).map(|p| p as usize)
    } else {
        Some(super::super::state::class_decl_prototype_object(class_id) as usize)
    };
    owner.is_some_and(|owner| {
        owner != 0
            && unsafe {
                crate::symbol::js_object_has_own_symbol(
                    crate::value::js_nanbox_pointer(owner as i64),
                    crate::value::js_nanbox_pointer(sym_key as i64),
                )
            }
    })
}

pub(crate) fn class_has_symbol_member_in_chain(
    class_id: u32,
    sym_key: usize,
    is_static: bool,
) -> bool {
    let mut cid = class_id;
    for _ in 0..32 {
        if cid == 0 {
            break;
        }
        if class_has_own_symbol_member(cid, sym_key, is_static) {
            return true;
        }
        match get_parent_class_id(cid) {
            Some(parent) if parent != cid => cid = parent,
            _ => break,
        }
    }
    false
}

/// Install a method on the prototype that owns this class evaluation. The
/// callable retains its evaluation home; [[Get]] and [[Call]] read the shape.
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn install_symbol_method(
    proto: *mut ObjectHeader,
    class_id: u32,
    owner: f64,
    key: f64,
    body: usize,
    params: u32,
    synthetic: bool,
    rest: bool,
    entry: usize,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto = scope.root_raw_mut_ptr(proto);
    let owner = scope.root_nanbox_f64(owner);
    let key = scope.root_nanbox_f64(key);
    let name =
        crate::symbol::symbol_function_name(crate::symbol::sym_key_from_f64(key.get_nanbox_f64()));
    let home = if is_class_object_value(owner.get_nanbox_f64()) {
        owner.get_nanbox_f64()
    } else {
        crate::object::class_prototype_ref_value(class_id)
    };
    let method = scope.root_nanbox_f64(
        crate::object::native_module::class_method_declaration_value(
            home, &name, body, params, synthetic, rest, entry, true,
        ),
    );
    crate::symbol::define_symbol_data_property(
        proto.with_mut_ptr::<ObjectHeader, _>(|p| crate::value::js_nanbox_pointer(p as i64)),
        key.get_nanbox_f64(),
        method.get_nanbox_f64(),
    );
    crate::symbol::set_symbol_property_attrs(
        proto.with_const_ptr::<ObjectHeader, _>(|p| p as usize),
        crate::symbol::sym_key_from_f64(key.get_nanbox_f64()),
        PropertyAttrs::new(true, false, true),
    );
}

pub(crate) unsafe fn install_symbol_accessor(
    class_id: u32,
    owner: f64,
    key: f64,
    raw_get: usize,
    raw_set: usize,
    is_static: bool,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let key = scope.root_nanbox_f64(key);
    let target = if is_static && !is_class_object_value(owner.get_nanbox_f64()) {
        crate::object::class_value::class_value(class_id)
    } else {
        owner.get_nanbox_f64()
    };
    let target = scope.root_nanbox_f64(target);
    let sym = crate::symbol::sym_key_from_f64(key.get_nanbox_f64());
    let name = crate::symbol::symbol_function_name(sym);
    let addr = crate::value::js_nanbox_get_pointer(target.get_nanbox_f64()) as usize;
    let (have_get, have_set) =
        crate::symbol::symbol_accessor_descriptor_bits(addr, sym).unwrap_or((0, 0));
    let get = scope.root_nanbox_u64(have_get);
    let set = scope.root_nanbox_u64(have_set);
    let get = if raw_get == 0 {
        get
    } else {
        scope.root_nanbox_f64(super::super::class_accessor_function_value(
            raw_get, false, is_static, &name, None,
        ))
    };
    let set = if raw_set == 0 {
        set
    } else {
        scope.root_nanbox_f64(super::super::class_accessor_function_value(
            raw_set, true, is_static, &name, None,
        ))
    };
    if is_static {
        crate::symbol::CLASS_STATIC_SYMBOLS_LATCH.arm();
    }
    crate::symbol::set_symbol_accessor_property(
        target.get_nanbox_f64(),
        key.get_nanbox_f64(),
        get.get_nanbox_u64(),
        set.get_nanbox_u64(),
    );
    crate::symbol::set_symbol_property_attrs(
        crate::value::js_nanbox_get_pointer(target.get_nanbox_f64()) as usize,
        crate::symbol::sym_key_from_f64(key.get_nanbox_f64()),
        PropertyAttrs::new(true, false, true),
    );
}

/// Install one symbol descriptor from this evaluation's key and immutable body.
/// Returning false leaves an ordinary string member to the same birth walk.
pub(crate) fn install_prototype_symbol_member(
    proto: &crate::gc::RuntimeHandle<'_>,
    class_id: u32,
    owner: &crate::gc::RuntimeHandle<'_>,
    member: &super::super::declarations::ClassMemberDecl,
) -> bool {
    if member.code.is_null() || (!member.name.is_null() && !member.is_symbol_named()) {
        return false;
    }
    let key = if let Some(sym) = member.symbol_key() {
        Some(crate::value::js_nanbox_pointer(sym as i64))
    } else if member.name.is_null() {
        if is_class_object_value(owner.get_nanbox_f64()) {
            unsafe {
                super::super::declarations::evaluation_computed_member_key(
                    crate::value::js_nanbox_get_pointer(owner.get_nanbox_f64())
                        as *const ObjectHeader,
                    class_id,
                    member,
                )
            }
        } else {
            super::super::declarations::computed_key_value(class_id, member.definition_order)
        }
    } else {
        None
    };
    let Some(key) = key.filter(|key| unsafe { crate::symbol::js_is_symbol(*key) != 0 }) else {
        return false;
    };
    unsafe {
        if member.is_method() {
            install_symbol_method(
                proto.get_raw_mut_ptr(),
                class_id,
                owner.get_nanbox_f64(),
                key,
                member.code as usize,
                member.param_count,
                member.has_synthetic_arguments(),
                member.has_rest(),
                member.entry as usize,
            );
        } else if member.is_accessor() {
            let getter = member.kind == super::super::declarations::CLASS_MEMBER_GETTER;
            install_symbol_accessor(
                class_id,
                proto
                    .with_mut_ptr::<ObjectHeader, _>(|p| crate::value::js_nanbox_pointer(p as i64)),
                key,
                if getter { member.code as usize } else { 0 },
                if getter { 0 } else { member.code as usize },
                false,
            );
        }
    }
    true
}

/// Birth stops where ClassBody evaluation has not named a member yet. The
/// declaration's existing resume walk installs that member and its successors
/// when their keys are ready, preserving symbol creation order on the shape.
pub(crate) fn install_prototype_symbol_members(
    proto: *mut ObjectHeader,
    class_id: u32,
    owner: f64,
) {
    let Some(decl) = super::super::declarations::class_declaration(class_id) else {
        return;
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto = scope.root_raw_mut_ptr(proto);
    let owner = scope.root_nanbox_f64(owner);
    let end =
        super::super::declarations::first_unnamed(class_id, decl).unwrap_or(decl.members().len());
    for member in &decl.members()[..end] {
        install_prototype_symbol_member(&proto, class_id, &owner, member);
    }
}

#[cfg(test)]
mod static_receiver_tests {
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
            for class_id in [own, deep] {
                crate::object::js_register_class_id(class_id);
            }
            let sym = crate::symbol::js_symbol_new_empty();
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

            let receiver = crate::object::class_value::class_value(deep);
            let got = crate::symbol::js_object_get_symbol_property(receiver, sym);
            assert_eq!(got.to_bits(), receiver.to_bits());
            let own_receiver = crate::object::class_value::class_value(own);
            let got_own = crate::symbol::js_object_get_symbol_property(own_receiver, sym);
            assert_eq!(got_own.to_bits(), own_receiver.to_bits());
            SETTER_CALL.with(|c| c.set(None));
            crate::symbol::js_object_set_symbol_property(receiver, sym, 7.0);
            assert_eq!(
                SETTER_CALL.with(|c| c.get()),
                Some((receiver.to_bits(), 7.0_f64.to_bits()))
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
