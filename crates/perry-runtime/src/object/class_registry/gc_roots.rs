use super::*;

#[derive(Clone)]
enum ClassSideTableRootSlot {
    DynamicProp {
        class_id: u32,
        name: String,
    },
    PrototypeMethod {
        class_id: u32,
        name: String,
    },
    PrototypeMethodValue {
        class_id: u32,
        name: String,
    },
    PrototypeObject {
        class_id: u32,
    },
    ParentClosure {
        class_id: u32,
    },
    DynamicParentValue {
        class_id: u32,
    },
    ClassObjectValue {
        class_id: u32,
    },
    ClassSymbolMethod {
        class_id: u32,
        sym_key: usize,
        is_static: bool,
    },
    ClassSymbolAccessor {
        class_id: u32,
        sym_key: usize,
        is_static: bool,
    },
    FunctionClassIdKey {
        bits: u64,
    },
}

pub(crate) struct ClassSideTableRootScanState {
    slots: Vec<ClassSideTableRootSlot>,
    cursor: usize,
}

pub(crate) fn new_class_side_table_root_scan_state() -> Box<dyn std::any::Any> {
    Box::new(ClassSideTableRootScanState {
        slots: class_side_table_root_snapshot(),
        cursor: 0,
    })
}

pub(crate) fn scan_class_side_table_roots_mut_step(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    state: &mut dyn std::any::Any,
    remaining: &mut usize,
) -> bool {
    let state = state
        .downcast_mut::<ClassSideTableRootScanState>()
        .expect("class side-table root scanner state type");
    while *remaining > 0 && state.cursor < state.slots.len() {
        scan_class_side_table_root_slot(visitor, &state.slots[state.cursor]);
        state.cursor += 1;
        *remaining -= 1;
    }
    state.cursor >= state.slots.len()
}

pub fn scan_class_side_table_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_class_side_table_roots_mut(&mut visitor);
}

pub fn scan_class_side_table_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    CLASS_DYNAMIC_PROPS.with(|m| {
        let mut m = m.borrow_mut();
        for props in m.values_mut() {
            for value in props.values_mut() {
                visitor.visit_nanbox_f64_slot(value);
            }
        }
    });

    CLASS_PROTOTYPE_METHODS.with(|table| {
        if let Ok(mut guard) = table.write() {
            if let Some(map) = guard.as_mut() {
                for methods in map.values_mut() {
                    for value_bits in methods.values_mut() {
                        visitor.visit_nanbox_u64_slot(value_bits);
                    }
                }
            }
        }
    });

    CLASS_PROTOTYPE_METHOD_VALUES.with(|cache| {
        let mut cache = cache.borrow_mut();
        for value_bits in cache.values_mut() {
            visitor.visit_nanbox_u64_slot(value_bits);
        }
    });

    CLASS_PROTOTYPE_OBJECTS.with(|table| {
        if let Ok(mut guard) = table.write() {
            if let Some(map) = guard.as_mut() {
                for proto_addr in map.values_mut() {
                    visitor.visit_usize_slot(proto_addr);
                }
            }
        }
    });

    CLASS_PARENT_CLOSURES.with(|table| {
        if let Ok(mut guard) = table.write() {
            if let Some(map) = guard.as_mut() {
                for closure_addr in map.values_mut() {
                    visitor.visit_usize_slot(closure_addr);
                }
            }
        }
    });

    // The dynamic-parent value stash (`class X extends _mod.default`) holds
    // raw NaN-boxed parent-constructor bits. For a ClassRef (INT32-tagged)
    // parent this is inert, but a function/object parent (Effect's
    // `extends <runtime value>`) is a live heap pointer that a moving GC must
    // visit + forward — otherwise `js_get_dynamic_parent_value` later hands
    // `super()` a stale pointer.
    CLASS_DYNAMIC_PARENT_VALUE.with(|table| {
        if let Ok(mut guard) = table.write() {
            if let Some(map) = guard.as_mut() {
                for value_bits in map.values_mut() {
                    visitor.visit_nanbox_u64_slot(value_bits);
                }
            }
        }
    });

    // #6530: cid → per-evaluation class OBJECT (`instance.constructor`
    // identity for capture-carrying classes). Same liveness/forwarding needs
    // as the dynamic-parent stash above: the entries are heap objects a
    // moving GC must visit + forward.
    CLASS_OBJECT_VALUES.with(|table| {
        if let Ok(mut guard) = table.write() {
            if let Some(map) = guard.as_mut() {
                for value_bits in map.values_mut() {
                    visitor.visit_nanbox_u64_slot(value_bits);
                }
            }
        }
    });

    scan_class_symbol_member_keys_mut(visitor);
    scan_function_class_id_keys_mut(visitor);
}

fn scan_class_symbol_member_keys_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    CLASS_SYMBOL_METHODS.with(|table| {
        if let Ok(mut guard) = table.write() {
            if let Some(map) = guard.as_mut() {
                let mut rewrites = Vec::new();
                for key in map.keys().copied().collect::<Vec<_>>() {
                    let (class_id, sym_key, is_static) = key;
                    let mut new_sym_key = sym_key;
                    if visitor.visit_usize_slot(&mut new_sym_key) && new_sym_key != sym_key {
                        rewrites.push((key, (class_id, new_sym_key, is_static)));
                    }
                }
                for (old_key, new_key) in rewrites {
                    if let Some(entry) = map.remove(&old_key) {
                        map.insert(new_key, entry);
                    }
                }
            }
        }
    });
    CLASS_SYMBOL_ACCESSORS.with(|table| {
        if let Ok(mut guard) = table.write() {
            if let Some(map) = guard.as_mut() {
                let mut rewrites = Vec::new();
                for key in map.keys().copied().collect::<Vec<_>>() {
                    let (class_id, sym_key, is_static) = key;
                    let mut new_sym_key = sym_key;
                    if visitor.visit_usize_slot(&mut new_sym_key) && new_sym_key != sym_key {
                        rewrites.push((key, (class_id, new_sym_key, is_static)));
                    }
                }
                for (old_key, new_key) in rewrites {
                    if let Some(entry) = map.remove(&old_key) {
                        map.insert(new_key, entry);
                    }
                }
            }
        }
    });
}

fn class_side_table_root_snapshot() -> Vec<ClassSideTableRootSlot> {
    let mut slots = Vec::new();

    CLASS_DYNAMIC_PROPS.with(|m| {
        let m = m.borrow();
        for (&class_id, props) in m.iter() {
            for name in props.keys() {
                slots.push(ClassSideTableRootSlot::DynamicProp {
                    class_id,
                    name: name.clone(),
                });
            }
        }
    });

    CLASS_PROTOTYPE_METHODS.with(|table| {
        if let Ok(guard) = table.read() {
            if let Some(map) = guard.as_ref() {
                for (&class_id, methods) in map.iter() {
                    for name in methods.keys() {
                        slots.push(ClassSideTableRootSlot::PrototypeMethod {
                            class_id,
                            name: name.clone(),
                        });
                    }
                }
            }
        }
    });

    CLASS_PROTOTYPE_METHOD_VALUES.with(|cache| {
        let cache = cache.borrow();
        for ((class_id, name), _) in cache.iter() {
            slots.push(ClassSideTableRootSlot::PrototypeMethodValue {
                class_id: *class_id,
                name: name.clone(),
            });
        }
    });

    CLASS_PROTOTYPE_OBJECTS.with(|table| {
        if let Ok(guard) = table.read() {
            if let Some(map) = guard.as_ref() {
                for &class_id in map.keys() {
                    slots.push(ClassSideTableRootSlot::PrototypeObject { class_id });
                }
            }
        }
    });

    CLASS_PARENT_CLOSURES.with(|table| {
        if let Ok(guard) = table.read() {
            if let Some(map) = guard.as_ref() {
                for &class_id in map.keys() {
                    slots.push(ClassSideTableRootSlot::ParentClosure { class_id });
                }
            }
        }
    });

    // Step twin of the CLASS_DYNAMIC_PARENT_VALUE block in
    // `scan_class_side_table_roots_mut`. Cycle-based collections run only
    // this snapshot machine, so omitting the stash meant a heap parent
    // (`class X extends someRuntimeValue()`) reachable only through it was
    // swept/left stale — `super()` then dereferenced freed/moved memory.
    CLASS_DYNAMIC_PARENT_VALUE.with(|table| {
        if let Ok(guard) = table.read() {
            if let Some(map) = guard.as_ref() {
                for &class_id in map.keys() {
                    slots.push(ClassSideTableRootSlot::DynamicParentValue { class_id });
                }
            }
        }
    });

    // Step twin of the CLASS_OBJECT_VALUES block in
    // `scan_class_side_table_roots_mut` (#6530).
    CLASS_OBJECT_VALUES.with(|table| {
        if let Ok(guard) = table.read() {
            if let Some(map) = guard.as_ref() {
                for &class_id in map.keys() {
                    slots.push(ClassSideTableRootSlot::ClassObjectValue { class_id });
                }
            }
        }
    });

    CLASS_SYMBOL_METHODS.with(|table| {
        if let Ok(guard) = table.read() {
            if let Some(map) = guard.as_ref() {
                for &(class_id, sym_key, is_static) in map.keys() {
                    slots.push(ClassSideTableRootSlot::ClassSymbolMethod {
                        class_id,
                        sym_key,
                        is_static,
                    });
                }
            }
        }
    });

    CLASS_SYMBOL_ACCESSORS.with(|table| {
        if let Ok(guard) = table.read() {
            if let Some(map) = guard.as_ref() {
                for &(class_id, sym_key, is_static) in map.keys() {
                    slots.push(ClassSideTableRootSlot::ClassSymbolAccessor {
                        class_id,
                        sym_key,
                        is_static,
                    });
                }
            }
        }
    });

    FUNCTION_CLASS_IDS.with(|table| {
        if let Ok(guard) = table.read() {
            if let Some(map) = guard.as_ref() {
                for &bits in map.keys() {
                    slots.push(ClassSideTableRootSlot::FunctionClassIdKey { bits });
                }
            }
        }
    });

    slots
}

fn scan_class_side_table_root_slot(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    slot: &ClassSideTableRootSlot,
) {
    match slot {
        ClassSideTableRootSlot::DynamicProp { class_id, name } => {
            CLASS_DYNAMIC_PROPS.with(|m| {
                if let Some(value) = m
                    .borrow_mut()
                    .get_mut(class_id)
                    .and_then(|props| props.get_mut(name))
                {
                    visitor.visit_nanbox_f64_slot(value);
                }
            });
        }
        ClassSideTableRootSlot::PrototypeMethod { class_id, name } => {
            CLASS_PROTOTYPE_METHODS.with(|table| {
                if let Ok(mut guard) = table.write() {
                    if let Some(value_bits) = guard
                        .as_mut()
                        .and_then(|map| map.get_mut(class_id))
                        .and_then(|methods| methods.get_mut(name))
                    {
                        visitor.visit_nanbox_u64_slot(value_bits);
                    }
                }
            });
        }
        ClassSideTableRootSlot::PrototypeMethodValue { class_id, name } => {
            CLASS_PROTOTYPE_METHOD_VALUES.with(|cache| {
                if let Some(value_bits) = cache.borrow_mut().get_mut(&(*class_id, name.clone())) {
                    visitor.visit_nanbox_u64_slot(value_bits);
                }
            });
        }
        ClassSideTableRootSlot::PrototypeObject { class_id } => {
            CLASS_PROTOTYPE_OBJECTS.with(|table| {
                if let Ok(mut guard) = table.write() {
                    if let Some(proto_addr) = guard.as_mut().and_then(|map| map.get_mut(class_id)) {
                        visitor.visit_usize_slot(proto_addr);
                    }
                }
            });
        }
        ClassSideTableRootSlot::ParentClosure { class_id } => {
            CLASS_PARENT_CLOSURES.with(|table| {
                if let Ok(mut guard) = table.write() {
                    if let Some(closure_addr) = guard.as_mut().and_then(|map| map.get_mut(class_id))
                    {
                        visitor.visit_usize_slot(closure_addr);
                    }
                }
            });
        }
        ClassSideTableRootSlot::DynamicParentValue { class_id } => {
            CLASS_DYNAMIC_PARENT_VALUE.with(|table| {
                if let Ok(mut guard) = table.write() {
                    if let Some(value_bits) = guard.as_mut().and_then(|map| map.get_mut(class_id)) {
                        visitor.visit_nanbox_u64_slot(value_bits);
                    }
                }
            });
        }
        ClassSideTableRootSlot::ClassObjectValue { class_id } => {
            CLASS_OBJECT_VALUES.with(|table| {
                if let Ok(mut guard) = table.write() {
                    if let Some(value_bits) = guard.as_mut().and_then(|map| map.get_mut(class_id)) {
                        visitor.visit_nanbox_u64_slot(value_bits);
                    }
                }
            });
        }
        ClassSideTableRootSlot::ClassSymbolMethod {
            class_id,
            sym_key,
            is_static,
        } => {
            rewrite_class_symbol_method_key_if_forwarded(visitor, *class_id, *sym_key, *is_static);
        }
        ClassSideTableRootSlot::ClassSymbolAccessor {
            class_id,
            sym_key,
            is_static,
        } => {
            rewrite_class_symbol_accessor_key_if_forwarded(
                visitor, *class_id, *sym_key, *is_static,
            );
        }
        ClassSideTableRootSlot::FunctionClassIdKey { bits } => {
            rewrite_function_class_id_key_if_forwarded(visitor, *bits);
        }
    }
}

fn rewrite_class_symbol_method_key_if_forwarded(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    class_id: u32,
    sym_key: usize,
    is_static: bool,
) {
    let mut new_sym_key = sym_key;
    if !visitor.visit_usize_slot(&mut new_sym_key) || new_sym_key == sym_key {
        return;
    }
    CLASS_SYMBOL_METHODS.with(|table| {
        if let Ok(mut guard) = table.write() {
            if let Some(map) = guard.as_mut() {
                if let Some(entry) = map.remove(&(class_id, sym_key, is_static)) {
                    map.insert((class_id, new_sym_key, is_static), entry);
                }
            }
        }
    });
}

fn rewrite_class_symbol_accessor_key_if_forwarded(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    class_id: u32,
    sym_key: usize,
    is_static: bool,
) {
    let mut new_sym_key = sym_key;
    if !visitor.visit_usize_slot(&mut new_sym_key) || new_sym_key == sym_key {
        return;
    }
    CLASS_SYMBOL_ACCESSORS.with(|table| {
        if let Ok(mut guard) = table.write() {
            if let Some(map) = guard.as_mut() {
                if let Some(entry) = map.remove(&(class_id, sym_key, is_static)) {
                    map.insert((class_id, new_sym_key, is_static), entry);
                }
            }
        }
    });
}

fn scan_function_class_id_keys_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    if !visitor.is_metadata_rewrite_phase() {
        return;
    }
    let mut rewrites = Vec::new();
    FUNCTION_CLASS_IDS.with(|table| {
        if let Ok(mut guard) = table.write() {
            let Some(map) = guard.as_mut() else {
                return;
            };
            for old_bits in map.keys().copied().collect::<Vec<_>>() {
                let mut new_bits = old_bits;
                if visit_metadata_nanbox_key(visitor, &mut new_bits) && new_bits != old_bits {
                    rewrites.push((old_bits, new_bits));
                }
            }
            for (old_bits, new_bits) in rewrites {
                if let Some(class_id) = map.remove(&old_bits) {
                    map.insert(new_bits, class_id);
                }
            }
        }
    });
}

fn rewrite_function_class_id_key_if_forwarded(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    old_bits: u64,
) {
    if !visitor.is_metadata_rewrite_phase() {
        return;
    }
    let mut new_bits = old_bits;
    if !visit_metadata_nanbox_key(visitor, &mut new_bits) || new_bits == old_bits {
        return;
    }
    FUNCTION_CLASS_IDS.with(|table| {
        if let Ok(mut guard) = table.write() {
            if let Some(map) = guard.as_mut() {
                if let Some(class_id) = map.remove(&old_bits) {
                    map.insert(new_bits, class_id);
                }
            }
        }
    });
}

/// #8040: death pruning for `FUNCTION_CLASS_IDS`.
///
/// The table maps a synthetic-class function value's NaN-boxed **bits** to a
/// class id, so its key is the raw heap address of a closure. Both writers
/// (`js_set_function_prototype`, `synthetic_class_id_for_function`) require a
/// `POINTER_TAG`'d value that passes `is_callable_function_value`, i.e. a key
/// can only ever have been a live `GC_TYPE_CLOSURE`.
///
/// Nothing told the table when that closure died, and this table — unlike the
/// other address-keyed side tables — is *rekeyed* rather than re-derived when
/// its key object moves (`scan_function_class_id_keys_mut`). So a dead key
/// does not merely leak: the arena recycles the address, the recycled bytes
/// are read as a `GcHeader`, and if the byte at the `gc_flags` offset happens
/// to carry `GC_FLAG_FORWARDED` the rekey walk follows the "forwarding
/// pointer" out of dead memory and binds the class id to whatever that word
/// names. See `gc::dead_owner`'s module doc for the general ABA hazard; this
/// table was simply never wired into its fan-out.
pub(crate) fn prune_dead_function_class_id_keys(is_dead_closure: &dyn Fn(usize) -> bool) {
    FUNCTION_CLASS_IDS.with(|table| {
        let Ok(mut guard) = table.write() else {
            return;
        };
        let Some(map) = guard.as_mut() else {
            return;
        };
        map.retain(|&bits, _| {
            if bits & crate::value::TAG_MASK != crate::value::POINTER_TAG {
                return true;
            }
            let addr = (bits & crate::value::POINTER_MASK) as usize;
            addr == 0 || !is_dead_closure(addr)
        });
    });
}

#[cfg(test)]
pub(crate) fn test_function_class_id_key_count() -> usize {
    FUNCTION_CLASS_IDS.with(|table| {
        table
            .read()
            .ok()
            .and_then(|guard| guard.as_ref().map(|map| map.len()))
            .unwrap_or(0)
    })
}

fn visit_metadata_nanbox_key(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    bits: &mut u64,
) -> bool {
    let tag = *bits & crate::value::TAG_MASK;
    if tag != crate::value::POINTER_TAG
        && tag != crate::value::STRING_TAG
        && tag != crate::value::BIGINT_TAG
    {
        return false;
    }
    let mut addr = (*bits & crate::value::POINTER_MASK) as usize;
    if visitor.visit_metadata_usize_slot(&mut addr) {
        *bits = tag | (addr as u64 & crate::value::POINTER_MASK);
        true
    } else {
        false
    }
}

#[cfg(test)]
pub(crate) fn test_clear_class_side_table_roots() {
    // Disambiguate: CLASS_DELETED_KEYS is reachable via both `use super::*`
    // and `use crate::object::*`; name the canonical definition explicitly.
    use super::state::CLASS_DELETED_KEYS;
    CLASS_DYNAMIC_PROPS.with(|m| m.borrow_mut().clear());
    CLASS_DELETED_KEYS.with(|m| m.borrow_mut().clear());
    CLASS_PROTOTYPE_METHOD_VALUES.with(|cache| cache.borrow_mut().clear());
    CLASS_PROTOTYPE_METHODS.with(|table| {
        if let Ok(mut guard) = table.write() {
            *guard = None;
        }
    });
    CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED.store(false, std::sync::atomic::Ordering::Release);
    FUNCTION_CLASS_IDS.with(|table| {
        if let Ok(mut guard) = table.write() {
            *guard = None;
        }
    });
    CLASS_PROTOTYPE_OBJECTS.with(|table| {
        if let Ok(mut guard) = table.write() {
            *guard = None;
        }
    });
    CLASS_PARENT_CLOSURES.with(|table| {
        if let Ok(mut guard) = table.write() {
            *guard = None;
        }
    });
    CLASS_SYMBOL_METHODS.with(|table| {
        if let Ok(mut guard) = table.write() {
            *guard = None;
        }
    });
    CLASS_SYMBOL_ACCESSORS.with(|table| {
        if let Ok(mut guard) = table.write() {
            *guard = None;
        }
    });
    if let Ok(mut guard) = CLASS_STATIC_ACCESSORS.write() {
        *guard = None;
    }
    NEXT_SYNTHETIC_CLASS_ID.store(0x8000_0000, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(test)]
pub(crate) fn test_seed_class_dynamic_prop_root(class_id: u32, name: &str, value_bits: u64) {
    class_dynamic_prop_root_store(class_id, name, f64::from_bits(value_bits));
}

#[cfg(test)]
pub(crate) fn test_class_dynamic_prop_root_bits(class_id: u32, name: &str) -> u64 {
    CLASS_DYNAMIC_PROPS.with(|m| {
        m.borrow()
            .get(&class_id)
            .and_then(|props| props.get(name))
            .map(|value| value.to_bits())
            .unwrap_or(0)
    })
}

#[cfg(test)]
pub(crate) fn test_seed_class_prototype_method_root(class_id: u32, name: &str, value_bits: u64) {
    class_prototype_method_root_store(class_id, name.to_string(), value_bits);
}

#[cfg(test)]
pub(crate) fn test_class_prototype_method_root_bits(class_id: u32, name: &str) -> u64 {
    CLASS_PROTOTYPE_METHODS.with(|table| {
        table
            .read()
            .ok()
            .and_then(|guard| {
                guard
                    .as_ref()
                    .and_then(|map| map.get(&class_id))
                    .and_then(|methods| methods.get(name))
                    .copied()
            })
            .unwrap_or(0)
    })
}

#[cfg(test)]
pub(crate) fn test_seed_class_prototype_method_value_root(
    class_id: u32,
    name: &str,
    value_bits: u64,
) {
    class_prototype_method_value_cache_root_store(class_id, name.to_string(), value_bits);
}

#[cfg(test)]
pub(crate) fn test_class_prototype_method_value_root_bits(class_id: u32, name: &str) -> u64 {
    CLASS_PROTOTYPE_METHOD_VALUES.with(|cache| {
        cache
            .borrow()
            .get(&(class_id, name.to_string()))
            .copied()
            .unwrap_or(0)
    })
}

#[cfg(test)]
pub(crate) fn test_seed_class_prototype_object_root(class_id: u32, addr: usize) {
    class_prototype_object_root_store(class_id, addr as *mut ObjectHeader);
}

#[cfg(test)]
pub(crate) fn test_class_prototype_object_root_addr(class_id: u32) -> usize {
    CLASS_PROTOTYPE_OBJECTS.with(|table| {
        table
            .read()
            .ok()
            .and_then(|guard| guard.as_ref().and_then(|map| map.get(&class_id).copied()))
            .unwrap_or(0)
    })
}

#[cfg(test)]
pub(crate) fn test_class_parent_closure_root_addr(class_id: u32) -> usize {
    CLASS_PARENT_CLOSURES.with(|table| {
        table
            .read()
            .ok()
            .and_then(|guard| guard.as_ref().and_then(|map| map.get(&class_id).copied()))
            .unwrap_or(0)
    })
}

#[cfg(test)]
pub(crate) fn test_seed_function_class_id_key(func_bits: u64, class_id: u32) {
    FUNCTION_CLASS_IDS.with(|table| {
        let mut guard = table.write().unwrap();
        if guard.is_none() {
            *guard = Some(HashMap::new());
        }
        guard.as_mut().unwrap().insert(func_bits, class_id);
    });
}

#[cfg(test)]
pub(crate) fn test_function_class_id_key_for_class(class_id: u32) -> u64 {
    FUNCTION_CLASS_IDS.with(|table| {
        table
            .read()
            .ok()
            .and_then(|guard| {
                guard.as_ref().and_then(|map| {
                    map.iter()
                        .find_map(|(&bits, &cid)| (cid == class_id).then_some(bits))
                })
            })
            .unwrap_or(0)
    })
}
