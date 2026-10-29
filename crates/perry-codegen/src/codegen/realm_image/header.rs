//! Header words of emitted realm cells. The emitter does not copy the
//! manifest's `_reserved` word: it derives the word with
//! `perry_abi::realm_header` from the cell it actually emits (the elements it
//! writes, its prototype slot, its members, its class) and requires the
//! manifest to agree. A disagreement names the cell: a declaration error or
//! a producer defect, never a word to emit.
//!
//! Facts no emitted cell shows (the layout classification a declared shape
//! owns, the prototype mark another birth's edge decides, per-instance
//! constructability) are read from the word itself, so the check also
//! rejects any bit outside the derivation's vocabulary.
use perry_abi::realm_header::{
    array_reserved, closure_reserved, members_need_descriptors, object_class_id, object_reserved,
    ArrayElements, ClosureBirth, Integrity, MemberAttrs, ObjectBirth, ObjectExotic, ObjectLayout,
    PrototypeMark,
};
use perry_hir::realm_manifest::{RealmDeclaration, RealmHeaderFacts, RealmValue};

use super::*;

fn agree(cell: &RealmDeclaration, manifest: u16, derived: u16) -> Result<u16, String> {
    if manifest == derived {
        Ok(derived)
    } else {
        Err(format!(
            "realm {}: manifest header word {manifest:#06x} disagrees with its derived birth word {derived:#06x}",
            cell.name
        ))
    }
}

fn is_pointer(value: &RealmValue) -> bool {
    matches!(
        value,
        RealmValue::String(_)
            | RealmValue::Symbol(_)
            | RealmValue::Reference(_)
            | RealmValue::RawReference(_)
            | RealmValue::Bytes(_)
    )
}

/// An array cell: the element kind of the slots it writes, its integrity.
pub(crate) fn array_word(
    cell: &RealmDeclaration,
    header: &RealmHeaderFacts,
    elements: &[&RealmValue],
) -> Result<u16, String> {
    let pointers = elements.iter().filter(|v| is_pointer(v)).count();
    let kind = if pointers == 0 {
        ArrayElements::Scalars
    } else if pointers == elements.len() {
        ArrayElements::Pointers
    } else {
        ArrayElements::Mixed
    };
    agree(
        cell,
        header.reserved,
        array_reserved(kind, Integrity::of_bits(header.reserved)),
    )
}

/// An object cell: its class, prototype slot, members and integrity.
pub(crate) fn object_word(
    cell: &RealmDeclaration,
    header: &RealmHeaderFacts,
) -> Result<u16, String> {
    let word = header.reserved;
    let class_id = header.class_id.unwrap_or(0);
    let exotic = if class_id == perry_abi::NATIVE_MODULE_CLASS_ID {
        ObjectExotic::NativeNamespace
    } else {
        ObjectExotic::Ordinary
    };
    if class_id == 0 || exotic == ObjectExotic::NativeNamespace {
        if object_class_id(exotic) != class_id {
            return Err(format!(
                "realm {}: class id disagrees with its birth",
                cell.name
            ));
        }
    }
    let members = cell
        .properties
        .iter()
        .map(|p| MemberAttrs {
            symbol: p.symbol,
            accessor: p.get.is_some() || p.set.is_some(),
            attributes: p.attributes,
        })
        .collect::<Vec<_>>();
    let birth = ObjectBirth {
        exotic,
        null_prototype: cell.prototype.is_none(),
        layout: if word & perry_abi::GC_OBJECT_CALLABLE != 0 {
            ObjectLayout::Callable
        } else if word & perry_abi::OBJ_FLAG_PLAIN_ORDINARY != 0 {
            ObjectLayout::PlainOrdinary
        } else {
            ObjectLayout::Unmarked
        },
        prototype_mark: if word & perry_abi::OBJ_FLAG_NEVER_MOVE != 0 {
            PrototypeMark::AtBirth
        } else {
            PrototypeMark::OnFirstLink
        },
        descriptors: members_need_descriptors(&members),
        typed_array_prototype: word & perry_abi::OBJ_FLAG_TYPED_ARRAY_PROTO != 0,
        integrity: Integrity::of_bits(word),
    };
    agree(cell, word, object_reserved(birth))
}

/// A closure cell's birth word (before the props-bag transition).
pub(crate) fn closure_word(
    cell: &RealmDeclaration,
    header: &RealmHeaderFacts,
) -> Result<u16, String> {
    let word = header.reserved;
    let birth = ClosureBirth {
        pointer_free_captures: word & perry_abi::GC_LAYOUT_POINTER_FREE != 0,
        constructable: word & perry_abi::GC_CLOSURE_NON_CONSTRUCTABLE == 0,
        body_non_constructor: false,
        integrity: Integrity::of_bits(word),
    };
    agree(cell, word, closure_reserved(birth))
}

/// The props bag an image closure is born with: a null-prototype plain
/// ordinary holder, marked at birth when it is a prototype holder.
pub(crate) fn bag_word(prototype: bool, descriptors: bool) -> u16 {
    object_reserved(ObjectBirth {
        exotic: ObjectExotic::Ordinary,
        null_prototype: true,
        layout: ObjectLayout::PlainOrdinary,
        prototype_mark: if prototype {
            PrototypeMark::AtBirth
        } else {
            PrototypeMark::OnFirstLink
        },
        descriptors,
        typed_array_prototype: false,
        integrity: Integrity::Extensible,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cells() -> Vec<RealmDeclaration> {
        crate::realm_manifest::REALM
            .iter()
            .map(|entry| serde_json::from_str(entry.facts).unwrap())
            .collect()
    }

    #[test]
    fn every_manifest_header_follows_from_its_cell() {
        let mut checked = 0;
        for cell in cells() {
            let Some(header) = &cell.header else { continue };
            let result = match header.gc_type {
                GC_TYPE_OBJECT => object_word(&cell, header),
                GC_TYPE_CLOSURE => closure_word(&cell, header),
                GC_TYPE_ARRAY => {
                    match super::super::arrays::ArrayParts::from_cell(&cell) {
                        Ok(parts) => array_word(&cell, header, &parts.elements),
                        // Provider-born arrays have no image elements.
                        Err(_) => continue,
                    }
                }
                _ => continue,
            };
            result.unwrap();
            checked += 1;
        }
        assert!(checked > 3000, "checked only {checked} cells");
    }

    #[test]
    fn a_disagreeing_word_names_the_cell() {
        let mut cell = cells()
            .into_iter()
            .find(|c| c.name == "node:http.METHODS")
            .unwrap();
        let header = cell.header.as_mut().unwrap();
        header.reserved = 0;
        let header = header.clone();
        let parts = super::super::arrays::ArrayParts::from_cell(&cell).unwrap();
        let error = array_word(&cell, &header, &parts.elements).unwrap_err();
        assert!(error.contains("node:http.METHODS"), "{error}");
    }
}
