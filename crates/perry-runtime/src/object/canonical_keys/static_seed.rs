//! Initialize image layouts from their compiler-computed key hashes. The
//! ordinary canonical trie remains the sole identity and lifetime authority.
use super::*;
use crate::object::layout_desc::PerryLayoutKey;

/// Initialize canonical keys using immutable image hashes and one trie walk.
/// Keep the generic initializer's allocation order, including duplicate lists:
/// this isolates the startup saving from changes to the program's heap layout.
/// Non-image and bound-symbol layouts retain the generic initializer.
///
/// # Safety
/// Descriptors and their atoms are valid, with hashes matching their strings.
/// GC must remain suppressed through this call, as for layout registration.
#[cold]
#[optimize(size)]
#[inline(never)]
pub(crate) unsafe fn from_static_layout(
    _proof: &SharedLayout,
    keys: &[PerryLayoutKey],
    mapped: bool,
) -> Option<CanonicalKeys> {
    debug_assert!(crate::gc::gc_is_suppressed());
    if keys.is_empty() {
        return Some(CanonicalKeys::EMPTY);
    }
    if keys.iter().any(|key| {
        key.atom.is_null()
            || (*key.atom).kind != crate::codegen_abi::ATOM_KIND_STRING
            || (*key.atom).string.is_null()
            || !crate::string::is_image_string((*key.atom).string)
    }) {
        return None;
    }
    let token = |key: &PerryLayoutKey| {
        u64::from(key.attrs)
            | if mapped {
                (u64::from(key.slot) + 1) << 8
            } else {
                0
            }
    };
    let count = keys.len() as u32;
    let fresh = crate::object::key_attrs::alloc_canonical_key_list(
        count,
        true,
        mapped || keys.iter().any(|key| key.attrs != 0),
    );
    let values = crate::array::array_elements_ptr(fresh);
    let attrs = crate::object::key_attrs::keys_attrs(fresh);
    for (i, key) in keys.iter().enumerate() {
        // GC_STORE_AUDIT(INIT): unpublished old backing, immutable image key;
        // no old-to-young edge is installed and collection is suppressed.
        *values.add(i) = JSValue::string_ptr((*key.atom).string.cast_mut()).bits();
        if !attrs.is_null() {
            crate::object::key_attrs::attrs_write(attrs, i as u32, key.attrs, &*key.atom);
            if mapped {
                crate::object::key_attrs::attrs_write_slot(attrs, i as u32, key.slot);
            }
        }
    }
    (*fresh).length = count;
    stamp_shared(fresh);
    Some(with_table_or(
        CanonicalKeys::new(fresh, count, NO_NODE),
        |t| {
            let mut parent = ROOT_NODE;
            let mut missing = false;
            for (i, key) in keys.iter().enumerate() {
                let atom = &*key.atom;
                let entry = token(key);
                let hash = edge_hash_with_entry(atom.hash, entry);
                if !missing {
                    if let Some(id) =
                        probe_node(t, parent, i as u32, Appended::Key(atom.string), entry, hash)
                    {
                        parent = id;
                        continue;
                    }
                    // An absent edge makes its entire suffix absent. Every edge
                    // before this point has already been checked exactly once.
                    missing = true;
                }
                parent = t.alloc_node(fresh as usize, parent, hash, i as u32 + 1, true, false);
            }
            let node = &t.nodes[parent as usize];
            if node.published {
                let existing = CanonicalKeys::new(node.addr as *mut ArrayHeader, count, parent);
                t.note_reached(parent);
                return existing;
            }
            t.publish(parent, fresh as usize, true);
            t.own_backing(parent, count);
            CanonicalKeys::new(fresh, count, parent)
        },
    ))
}

#[cfg(test)]
mod tests;

/// WRT step 2: link a canonical key list the compiler emitted as a cell of
/// the realm image into this agent's trie. `cell` is the user address of a
/// list holding exactly the strings of `keys`' atoms, in order; the
/// canonical list of that key sequence in this agent is then the cell, or
/// the list a receiver published first. `None` when a key is not an image
/// atom, or the cell does not hold its string: the caller mints by names.
///
/// # Safety
/// `keys` are this program's atom records and `cell` an image cell of
/// `keys.len()` slots. GC is suppressed, as for layout registration.
#[cold]
#[optimize(size)]
#[inline(never)]
pub(crate) unsafe fn link_image_list(
    _proof: &SharedLayout,
    keys: &[PerryLayoutKey],
    cell: *mut ArrayHeader,
) -> Option<CanonicalKeys> {
    debug_assert!(crate::gc::gc_is_suppressed());
    let count = keys.len() as u32;
    if keys.is_empty()
        || !crate::value::addr_class::realm::is_image_object(cell as usize)
        || (*cell).length != count
    {
        return None;
    }
    let slots = crate::array::array_elements_ptr(cell) as *const u64;
    let mapped = keys
        .iter()
        .enumerate()
        .any(|(i, key)| key.slot as usize != i);
    for (i, key) in keys.iter().enumerate() {
        if key.atom.is_null() {
            return None;
        }
        let atom = &*key.atom;
        if atom.kind == crate::codegen_abi::ATOM_KIND_BINDING_SYMBOL {
            return None;
        }
        if atom.kind == crate::codegen_abi::ATOM_KIND_STRING
            && (atom.string.is_null() || !crate::string::is_image_string(atom.string))
        {
            return None;
        }
        let expected = crate::object::layout_desc::layout_key_word(key, &[], &mut 0);
        if *slots.add(i) != expected {
            return None;
        }
        let entry = u64::from(key.attrs)
            | if mapped {
                (u64::from(key.slot) + 1) << 8
            } else {
                0
            };
        if crate::object::key_attrs::token_of_word(crate::object::key_attrs::key_word(
            cell, i as u32,
        )) != entry
        {
            return None;
        }
    }
    Some(with_table_or(
        CanonicalKeys::new(cell, count, NO_NODE),
        |t| {
            let mut parent = ROOT_NODE;
            let mut missing = false;
            for (i, key) in keys.iter().enumerate() {
                let atom = &*key.atom;
                let entry = u64::from(key.attrs)
                    | if mapped {
                        (u64::from(key.slot) + 1) << 8
                    } else {
                        0
                    };
                let appended = if atom.kind == crate::codegen_abi::ATOM_KIND_STRING {
                    Appended::Key(atom.string)
                } else {
                    Appended::Slot(JSValue::from_bits(*slots.add(i)))
                };
                let hash = if atom.kind == crate::codegen_abi::ATOM_KIND_STRING {
                    edge_hash_with_entry(atom.hash, entry)
                } else {
                    appended.edge_hash(entry)
                };
                if !missing {
                    if let Some(id) = probe_node(t, parent, i as u32, appended, entry, hash) {
                        parent = id;
                        continue;
                    }
                    missing = true;
                }
                parent = t.alloc_node(cell as usize, parent, hash, i as u32 + 1, true, false);
            }
            let node = &t.nodes[parent as usize];
            if node.published {
                let existing = CanonicalKeys::new(node.addr as *mut ArrayHeader, count, parent);
                t.note_reached(parent);
                return existing;
            }
            // The cell is image data: no backing was allocated for it.
            t.publish(parent, cell as usize, true);
            CanonicalKeys::new(cell, count, parent)
        },
    ))
}
