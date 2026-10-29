use super::*;

pub(crate) fn register_static_layouts() {
    #[cfg(target_family = "wasm")]
    assert!(
        image_layouts().next().is_some(),
        "Wasm layout constructors did not run"
    );
    unsafe {
        for (index, desc) in image_layouts().enumerate() {
            if (*desc).family == SENTINEL_FAMILY || (*desc).keys.is_null() && (*desc).key_count != 0
            {
                continue;
            }
            if (*desc).layout_id == 0 {
                continue;
            }
            // WRT step 2: a runtime layout or a module namespace draws its id
            // and installs its shape at its first birth
            // ([`layout_word_for_birth`], [`register_namespace_layout`]);
            // startup installs the program's own layouts only. The id cell is
            // process-wide, so every agent still sees one id per layout.
            if (*desc).variant == crate::codegen_abi::LAYOUT_VARIANT_NAMESPACE
                || is_runtime_layout_word((*desc).layout_id)
            {
                continue;
            }
            let Some(id) = layout_id_of_index(index as u32) else {
                break;
            };
            let _ = register_one(desc, id);
        }
    }
}

/// Is `word` a layout of the runtime's own image (the ABI's native rows)?
/// Generated code never names such a descriptor, so every object carrying
/// one is born by a runtime funnel ([`layout_word_for_birth`]), which draws
/// its id and installs its shape at the first birth.
#[inline]
pub(super) fn is_runtime_layout_word(word: u32) -> bool {
    word & crate::codegen_abi::LAYOUT_TAG != 0
        && LayoutWord(word).row().0 < crate::codegen_abi::NATIVE_LAYOUT_ROWS
}

/// The registered word of `desc` for a birth in this agent. A runtime
/// layout's shape is installed by its first birth here; every other
/// registered layout was installed at startup.
#[inline]
pub(crate) unsafe fn layout_word_for_birth(desc: *const PerryLayoutDesc) -> Option<u32> {
    let id = registered_layout_id(load_layout_id(desc))?;
    if is_runtime_layout_word(id)
        && super::super::shapes::shape_record_for_word(LayoutWord(id)).is_none()
    {
        install_runtime_layout(desc)?;
    }
    Some(id)
}

#[cold]
#[inline(never)]
unsafe fn install_runtime_layout(desc: *const PerryLayoutDesc) -> Option<()> {
    let requested = layout_id_of_index(descriptor_index(desc)?)?;
    let id = register_bound(desc, requested, &[], None);
    (id != 0 && super::super::shapes::shape_record_for_word(LayoutWord(id)).is_some()).then_some(())
}

#[cfg(any(test, debug_assertions))]
crate::perry_thread_local! {
    /// Runtime layouts whose shape this agent installed (debug and test
    /// builds only): startup installs none.
    pub(crate) static RUNTIME_LAYOUT_INSTALLS: std::cell::Cell<u32> =
        const { std::cell::Cell::new(0) };
}

/// What [`register_bound`] does with a descriptor that passed validation.
/// The installed layout of module namespace descriptor `desc`, installing
/// its key list and shape under the id startup reserved on the first birth
/// in this agent; `None` when this runtime cannot represent it.
pub(crate) unsafe fn register_namespace_layout(desc: *const PerryLayoutDesc) -> Option<u32> {
    let id = register_one(desc, 0);
    (id != 0).then_some(id)
}

/// The shape kind a descriptor's `kind` code declares, among the ordinary
/// kinds the registrar represents: `Ordinary`, and for a runtime prototype
/// whose stores must reach their funnel, `OrdinaryUnmarked` or
/// `CallableOrdinary`.
pub(super) fn declared_kind(code: u8) -> Option<super::super::shapes::ShapeObjectKind> {
    use super::super::shapes::ShapeObjectKind::{CallableOrdinary, Ordinary, OrdinaryUnmarked};
    [Ordinary, OrdinaryUnmarked, CallableOrdinary]
        .into_iter()
        .find(|k| k.code() == code as u64)
}

/// Register one descriptor under `requested`; the id it resolved to, or 0
/// when this runtime cannot represent it yet (a descriptor prototype, a
/// non-ordinary kind, slots that are not a permutation of the key positions,
/// a key that is not an atom). The key sequence is the enumeration order;
/// each key's slot is its storage.
pub(crate) unsafe fn register_one(desc: *const PerryLayoutDesc, requested: u32) -> u32 {
    register_one_bound(desc, requested, &[])
}

/// The module-local symbol keys (`ATOM_KIND_BINDING_SYMBOL`) of a
/// descriptor's key sequence.
pub(super) unsafe fn binding_symbol_keys(d: &PerryLayoutDesc) -> usize {
    if d.key_count == 0 {
        return 0;
    }
    std::slice::from_raw_parts(d.keys, d.key_count as usize)
        .iter()
        .filter(|k| (*k.atom).kind == crate::codegen_abi::ATOM_KIND_BINDING_SYMBOL)
        .count()
}

/// The key word of key `key` of a descriptor: a string's, a symbol atom's
/// symbol, or for the `j`-th module-local symbol key, `bound[j]` (`j` counts
/// those keys before it, `next` the running count).
pub(crate) unsafe fn layout_key_word(key: &PerryLayoutKey, bound: &[u64], next: &mut usize) -> u64 {
    if (*key.atom).kind == crate::codegen_abi::ATOM_KIND_BINDING_SYMBOL {
        let word = bound[*next];
        *next += 1;
        return word;
    }
    match atom_symbol(key.atom) {
        // The key word a symbol data property stores.
        Some(symbol) => crate::value::POINTER_TAG | symbol as u64,
        None => crate::value::JSValue::string_ptr((*key.atom).string.cast_mut()).bits(),
    }
}

/// [`register_one`] for a descriptor with module-local symbol keys: `bound`
/// holds their key words, in key order. Without them (startup) the id is
/// only reserved: the word names it, but no shape is installed until the
/// prototype's bind hands the symbols (`perry_bind_family_prototype`), so no
/// object carries the id before then.
pub(crate) unsafe fn register_one_bound(
    desc: *const PerryLayoutDesc,
    requested: u32,
    bound: &[u64],
) -> u32 {
    register_bound(desc, requested, bound, None)
}

/// An ordinary function's layout, with the actual prototype serial carried
/// by its ordinary shape record. The existing identity word roots its link.
pub(crate) unsafe fn register_function_layout(
    desc: *const PerryLayoutDesc,
    requested: u32,
    bound: &[u64],
    proto_id: u64,
) -> u32 {
    register_bound(desc, requested, bound, Some(proto_id))
}

pub(super) unsafe fn register_bound(
    desc: *const PerryLayoutDesc,
    requested: u32,
    bound: &[u64],
    function_proto: Option<u64>,
) -> u32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    let word = AtomicU32::from_ptr(std::ptr::addr_of!((*desc).shape_id).cast_mut());
    let function = (*desc).variant == crate::codegen_abi::LAYOUT_VARIANT_FUNCTION_CONSTRUCTOR;
    if function && function_proto.is_none() {
        return 0;
    }
    let existing = if function {
        None
    } else {
        Some(word.load(Ordering::Acquire)).filter(|&id| id != 0)
    };
    let local = binding_symbol_keys(&*desc);
    if let Some(id) = existing {
        if (local != 0 && bound.is_empty())
            || super::super::shapes::shape_descriptor_by_id(ShapeId(id)).is_some()
        {
            return (*desc).layout_id;
        }
    }
    if local != 0 && !bound.is_empty() && bound.len() != local {
        return 0;
    }
    let d = &*desc;
    let Some(kind) = declared_kind(d.kind) else {
        return 0;
    };
    if existing.is_none() && !function && requested >= super::super::shapes::STATIC_SHAPE_ID_END {
        return 0;
    }
    let all_keys: &[PerryLayoutKey] = if d.key_count == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(d.keys, d.key_count as usize)
    };
    // An optional key (a class family's late key, case 2c, or a runtime
    // layout's optional member) is in the birth key list with
    // `key_attrs::ENTRY_OPTIONAL`: absent while its slot holds the hole
    // (`key_view`).
    let keys = all_keys;
    // Names, hashes and attribute bytes already live in the binary.
    // Validate the fixed ABI without rebuilding transient name/attribute
    // vectors or revalidating each image string as UTF-8 at agent startup.
    let mapped = keys
        .iter()
        .enumerate()
        .any(|(i, key)| key.slot as usize != i);
    if mapped {
        // A permutation keeps the next position free as the next key's slot.
        let mut taken = vec![false; keys.len()];
        for key in keys {
            let slot = key.slot as usize;
            if slot >= keys.len() || taken[slot] {
                return 0;
            }
            taken[slot] = true;
        }
    }
    for key in keys {
        if key.slot >= d.slot_count {
            return 0;
        }
        match (*key.atom).kind {
            crate::codegen_abi::ATOM_KIND_STRING if !(*key.atom).string.is_null() => {}
            crate::codegen_abi::ATOM_KIND_WELL_KNOWN_SYMBOL
                if ((*key.atom).index as usize) < crate::codegen_abi::WELL_KNOWN_SYMBOLS.len() => {}
            // A symbol of node's own library the runtime binds
            // (`runtime_layouts::runtime_symbol_of_atom`).
            crate::codegen_abi::ATOM_KIND_PROGRAM_SYMBOL
                if super::super::runtime_layouts::runtime_symbol_of_atom(key.atom).is_some() => {}
            // A registered symbol (`Symbol.for(desc)`) the program names.
            crate::codegen_abi::ATOM_KIND_REGISTERED_SYMBOL if !(*key.atom).string.is_null() => {}
            // A module-local symbol: a prototype image's, bound at its bind.
            crate::codegen_abi::ATOM_KIND_BINDING_SYMBOL
                if d.variant == PROTOTYPE_IMAGE_VARIANT
                    || d.variant == crate::codegen_abi::LAYOUT_VARIANT_FUNCTION_CONSTRUCTOR => {}
            _ => return 0,
        }
    }
    // Case 2c: a class family's instances have the class's prototype, and
    // its id is its descriptor record's.
    let class_family = d.class_id != 0;
    let proto_id = match d.proto.addr() {
        3 => super::super::shapes::PROTO_ID_DEFAULT,
        _ if class_family => super::super::shapes::class_proto_id(d.class_id),
        0 => super::super::shapes::PROTO_ID_DEFAULT,
        1 => super::super::shapes::PROTO_ID_NULL,
        2 if d.variant == crate::codegen_abi::LAYOUT_VARIANT_FUNCTION_CONSTRUCTOR => {
            match function_proto {
                Some(id) if id != 0 => id,
                _ => return 0,
            }
        }
        fam if fam >= LAYOUT_PROTO_TAG_END => proto_family_serial(d.proto),
        _ => return 0,
    };
    if local != 0 && bound.is_empty() {
        // Reserve the id the registration would take; the bind installs it.
        let proposed = if proto_id == super::super::shapes::PROTO_ID_DEFAULT
            || d.variant == crate::codegen_abi::LAYOUT_VARIANT_FUNCTION_CONSTRUCTOR
        {
            requested
        } else {
            super::super::shapes::allocate_declared_layout_id(proto_id).0
        };
        return match word.compare_exchange(0, proposed, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => (*desc).layout_id,
            Err(_) => (*desc).layout_id,
        };
    }
    #[cfg(any(test, debug_assertions))]
    if is_runtime_layout_word(d.layout_id) {
        RUNTIME_LAYOUT_INSTALLS.with(|n| n.set(n.get() + 1));
    }
    let key_count = keys.len() as u32;
    let slot_count = d.slot_count;
    let rep = d.rep;
    let integrity = d.integrity as u16;
    // Compatibility key arrays are owned by the one authoritative shape
    // record, and are shared by the canonical key trie when names/attrs agree.
    let _no_gc = crate::gc::GcSuppressScope::new();
    // The canonical handle's view: the list and its trie node, which the
    // shape record carries (CKI S1).
    let list_keys = if key_count == 0 {
        super::super::ObjectKeys::NONE
    } else if super::super::image_keys::prepare(keys, d.image_keys) {
        super::super::canonical_keys::static_seed::link_image_list(
            &super::super::canonical_keys::SharedLayout::shape_cache_entry(),
            keys,
            d.image_keys,
        )
        .expect("descriptor image list agrees with its keys and attributes")
        .view()
    } else if let Some(canonical) = (!function)
        .then(|| {
            super::super::canonical_keys::from_static_layout(
                &super::super::canonical_keys::SharedLayout::shape_cache_entry(),
                keys,
                mapped,
            )
        })
        .flatten()
    {
        canonical.view()
    } else {
        let list = super::super::key_attrs::alloc_canonical_key_list(
            key_count,
            true,
            mapped || keys.iter().any(|key| key.attrs != 0),
        );
        let values = crate::array::array_elements_ptr(list);
        let attributes = super::super::key_attrs::keys_attrs(list);
        let mut next = 0;
        for (i, key) in keys.iter().enumerate() {
            // GC_STORE_AUDIT(INIT): unpublished list, under suppression.
            *values.add(i) = if function && (*key.atom).kind == crate::codegen_abi::ATOM_KIND_STRING
            {
                let atom = crate::string::property_atom((*key.atom).string);
                crate::value::JSValue::string_ptr(atom.cast_mut()).bits()
            } else {
                layout_key_word(key, bound, &mut next)
            };
            if !attributes.is_null() {
                super::super::key_attrs::attrs_write(attributes, i as u32, key.attrs, &*key.atom);
                if mapped {
                    super::super::key_attrs::attrs_write_slot(attributes, i as u32, key.slot);
                }
            }
        }
        (*list).length = key_count;
        super::super::canonical_keys::canonicalize_owned(
            &super::super::canonical_keys::SharedLayout::shape_cache_entry(),
            list,
            key_count,
        )
        .view()
    };
    let list = list_keys.arr();
    let summary = super::super::key_attrs::keys_summary(list, key_count);
    // A carried prototype layout is a marked prototype's: its own semantic
    // generation, shared with no other layout.
    let image = d.variant == PROTOTYPE_IMAGE_VARIANT;
    let generation = if image {
        super::super::shapes::fresh_semantic_generation()
    } else {
        0
    };
    if function {
        let word = super::super::shapes::publish_shape_result(
            super::super::shapes::shape_descriptor_intern_with_rep(
                list_keys,
                key_count,
                slot_count,
                generation,
                kind,
                0,
                proto_id,
                summary,
                rep,
                &[],
                None,
                integrity,
            ),
        );
        // A per-evaluation record is NOT an external carrier: the objects born
        // on it (and the prototype they name) keep it live, and it retires
        // with the last of them. The caller holds GC suppression from here to
        // the birth's stamp (function_family_birth::allocate_layout).
        return word.0;
    }
    let proposed = if let Some(id) = existing {
        id
    } else if function {
        super::super::shapes::allocate_declared_layout_id(proto_id).0
    } else if proto_id == super::super::shapes::PROTO_ID_DEFAULT
        || class_family
        || d.variant == crate::codegen_abi::LAYOUT_VARIANT_FUNCTION_CONSTRUCTOR
    {
        requested
    } else {
        super::super::shapes::allocate_declared_layout_id(proto_id).0
    };
    // End the descriptor borrow before the startup-only id publication.
    let id = if let Some(id) = existing {
        id
    } else {
        match word.compare_exchange(0, proposed, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => proposed,
            Err(other) => {
                assert!(
                    super::super::shapes::is_shape_id(other),
                    "descriptor identity cell holds a ShapeId"
                );
                other
            }
        }
    };
    if super::super::shapes::shape_descriptor_by_id(ShapeId(id)).is_none() {
        super::super::shapes::install_declared_layout(
            ShapeId(id),
            LayoutWord((*desc).layout_id),
            if (*desc).class_id != 0 || literal_birth::contains(desc) {
                descriptor_index(desc).map_or(0, |i| i + 1)
            } else {
                0
            },
            list_keys,
            key_count,
            slot_count,
            kind,
            proto_id,
            summary,
            rep,
            integrity,
            generation,
        );
    }
    super::super::shapes::note_external_shape_carrier(
        super::super::shapes::shape_descriptor_for_word(LayoutWord((*desc).layout_id)),
    );
    (*desc).layout_id
}
