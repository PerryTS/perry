//! The GC header words of a realm birth, derived from its declared facts.
//!
//! One derivation per GC kind. The realm manifest producer, the runtime birth
//! helpers that allocate realm cells, and the realm-image emitter all call
//! these functions; none of them computes a header bit on its own. A recorded
//! header that disagrees with the derivation is either a declaration error or
//! a defect of the recording birth path (WRT3a3, `DESIGN.md` §3.1).
//!
//! The inputs are birth facts only: what the birth holds, how it is linked and
//! how it is born. Nothing here depends on the process that runs the birth
//! (its argv, environment, host files) or on the collector's state (age,
//! marks, remembered sets): those never belong in a realm header.

use crate::{
    GC_LAYOUT_ALL_POINTERS, GC_LAYOUT_POINTER_FREE, GC_LAYOUT_SIDE_MASK, GC_OBJECT_CALLABLE,
    OBJ_FLAG_FROZEN, OBJ_FLAG_HAS_DESCRIPTORS, OBJ_FLAG_NEVER_MOVE, OBJ_FLAG_NO_EXTEND,
    OBJ_FLAG_NULL_PROTO, OBJ_FLAG_PLAIN_ORDINARY, OBJ_FLAG_SEALED,
};

/// `gc::OBJ_FLAG_TYPED_ARRAY_PROTO`: a per-kind `<TypedArray>.prototype`.
pub const OBJ_FLAG_TYPED_ARRAY_PROTO: u16 = 0x100;
/// `gc::GC_CLOSURE_NON_CONSTRUCTABLE`: per-instance builtin
/// non-constructability, kind-disjoint from the object meaning of bit 7.
pub const GC_CLOSURE_NON_CONSTRUCTABLE: u16 = 0x80;
/// `object::native_module::NATIVE_MODULE_CLASS_ID`: the class id of a native
/// module namespace, whose members a vtable serves.
pub const NATIVE_MODULE_CLASS_ID: u32 = 0xFFFF_FFFE;

/// The integrity level a birth leaves, as `Object.preventExtensions`,
/// `Object.seal` and `Object.freeze` do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Integrity {
    Extensible,
    NonExtensible,
    Sealed,
    Frozen,
}

impl Integrity {
    /// The header bits of this level. Each level includes the weaker ones.
    pub const fn bits(self) -> u16 {
        match self {
            Integrity::Extensible => 0,
            Integrity::NonExtensible => OBJ_FLAG_NO_EXTEND,
            Integrity::Sealed => OBJ_FLAG_SEALED | OBJ_FLAG_NO_EXTEND,
            Integrity::Frozen => OBJ_FLAG_FROZEN | OBJ_FLAG_SEALED | OBJ_FLAG_NO_EXTEND,
        }
    }

    /// Sealing and freezing make every own property non-configurable, so
    /// the holder carries descriptor attributes from birth.
    const fn installs_descriptors(self) -> bool {
        matches!(self, Integrity::Sealed | Integrity::Frozen)
    }
}

/// What the dense element prefix of an array birth may hold. A static birth
/// derives it from its declared elements (no element: `Scalars`); a dynamic
/// birth (`value = dynamic`) declares it, because its contents exist only at
/// run time and the header must not depend on them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrayElements {
    /// No element is a heap pointer: the empty birth, numbers, booleans.
    Scalars,
    /// Every element is a heap pointer (strings, objects).
    Pointers,
    /// Both kinds: the collector tests each slot's tag.
    Mixed,
}

/// The `_reserved` word of an array birth.
///
/// The layout state is the most precise one the declared elements prove:
/// `POINTER_FREE` for scalars, the all-pointer side layout for pointers, and
/// `UNKNOWN` (tag-scanned) only for a mixed prefix. A birth never claims the
/// raw-f64 representation: that bit is the generic `[]` allocator's guess at
/// future numeric pushes, not a fact of what the realm declares.
pub const fn array_reserved(elements: ArrayElements, integrity: Integrity) -> u16 {
    let layout = match elements {
        ArrayElements::Scalars => GC_LAYOUT_POINTER_FREE,
        ArrayElements::Pointers => GC_LAYOUT_SIDE_MASK | GC_LAYOUT_ALL_POINTERS,
        ArrayElements::Mixed => 0,
    };
    layout | integrity.bits()
}

/// How an object birth is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectExotic {
    /// Shape-keyed own storage.
    Ordinary,
    /// A native module namespace: its members and their descriptors live
    /// behind the native module vtable, not in header bits.
    NativeNamespace,
}

/// The layout an object birth is allocated from, which decides the
/// classification bits its header carries from birth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectLayout {
    /// A dictionary birth: no declared shape classifies its stores.
    Unmarked,
    /// A declared shape that proves ordinary `[[Set]]` semantics.
    PlainOrdinary,
    /// A declared callable layout (`LAYOUT_KIND_CALLABLE`).
    Callable,
}

/// When an object is marked as a prototype, which is what makes it
/// never-move (`proto_validity`, `gc::never_move`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrototypeMark {
    /// Marked at birth: a prototype factory birth, the realm global, a sealed
    /// builtin namespace, a declared `never_move`, or an object that another
    /// realm birth names as its `[[Prototype]]`.
    AtBirth,
    /// Marked when the first instance links to it, after the realm is built.
    OnFirstLink,
}

/// The declared facts of an object birth that decide its header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectBirth {
    pub exotic: ObjectExotic,
    /// The `[[Prototype]]` is null.
    pub null_prototype: bool,
    pub layout: ObjectLayout,
    pub prototype_mark: PrototypeMark,
    /// Some own string-keyed property is an accessor or has non-default
    /// attributes ([`members_need_descriptors`]).
    pub descriptors: bool,
    /// A per-kind `<TypedArray>.prototype`.
    pub typed_array_prototype: bool,
    pub integrity: Integrity,
}

/// One own property as [`members_need_descriptors`] reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemberAttrs {
    pub symbol: bool,
    pub accessor: bool,
    /// Public attribute bits: writable 1, enumerable 2, configurable 4.
    pub attributes: u8,
}

/// The descriptor fact of an ordinary holder: some own string-keyed property
/// is an accessor or has non-default attributes. This is the meaning of
/// `OBJ_FLAG_HAS_DESCRIPTORS` that every attribute install funnel publishes.
pub const fn members_need_descriptors(members: &[MemberAttrs]) -> bool {
    let mut i = 0;
    while i < members.len() {
        let m = members[i];
        if !m.symbol && (m.accessor || m.attributes != 7) {
            return true;
        }
        i += 1;
    }
    false
}

/// The `_reserved` word of an object birth.
///
/// The prototype mark is a fact of the cell, whatever serves its members: a
/// sealed builtin namespace served by the native vtable (`console`) is
/// never-move from birth like an ordinary one.
pub const fn object_reserved(birth: ObjectBirth) -> u16 {
    let integrity = birth.integrity.bits();
    let descriptors = birth.descriptors || birth.integrity.installs_descriptors();
    let never_move = if matches!(birth.prototype_mark, PrototypeMark::AtBirth) {
        OBJ_FLAG_NEVER_MOVE
    } else {
        0
    };
    match birth.exotic {
        ObjectExotic::NativeNamespace => {
            integrity
                | never_move
                | if birth.integrity.installs_descriptors() {
                    OBJ_FLAG_HAS_DESCRIPTORS
                } else {
                    0
                }
        }
        ObjectExotic::Ordinary => {
            integrity
                | never_move
                | if birth.null_prototype {
                    OBJ_FLAG_NULL_PROTO
                } else {
                    0
                }
                | if birth.typed_array_prototype {
                    OBJ_FLAG_TYPED_ARRAY_PROTO
                } else {
                    0
                }
                | match birth.layout {
                    ObjectLayout::Unmarked => 0,
                    ObjectLayout::PlainOrdinary => OBJ_FLAG_PLAIN_ORDINARY,
                    ObjectLayout::Callable => GC_OBJECT_CALLABLE,
                }
                | if descriptors {
                    OBJ_FLAG_HAS_DESCRIPTORS
                } else {
                    0
                }
        }
    }
}

/// The class id word of an object birth that has no class of its own.
pub const fn object_class_id(exotic: ObjectExotic) -> u32 {
    match exotic {
        ObjectExotic::Ordinary => 0,
        ObjectExotic::NativeNamespace => NATIVE_MODULE_CLASS_ID,
    }
}

/// The declared facts of a closure birth that decide its header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClosureBirth {
    /// No capture slot holds a heap pointer.
    pub pointer_free_captures: bool,
    /// `new` may call it. A constructor kind is constructable.
    pub constructable: bool,
    /// The body's info carries `FN_NON_CONSTRUCTOR`, which already refuses
    /// `new` for every closure of that body.
    pub body_non_constructor: bool,
    pub integrity: Integrity,
}

/// The `_reserved` word of a closure birth. The per-instance
/// non-constructable bit is set only where the body does not already say so.
pub const fn closure_reserved(birth: ClosureBirth) -> u16 {
    let layout = if birth.pointer_free_captures {
        GC_LAYOUT_POINTER_FREE
    } else {
        0
    };
    let non_constructable = !birth.constructable && !birth.body_non_constructor;
    layout
        | if non_constructable {
            GC_CLOSURE_NON_CONSTRUCTABLE
        } else {
            0
        }
        | birth.integrity.bits()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integrity_levels_nest() {
        assert_eq!(Integrity::Frozen.bits(), 7);
        assert_eq!(Integrity::Sealed.bits(), 6);
        assert_eq!(Integrity::NonExtensible.bits(), 4);
    }

    #[test]
    fn arrays_never_claim_raw_f64() {
        for elements in [
            ArrayElements::Scalars,
            ArrayElements::Pointers,
            ArrayElements::Mixed,
        ] {
            assert_eq!(array_reserved(elements, Integrity::Frozen) & 0x80, 0);
        }
    }

    #[test]
    fn native_namespaces_keep_descriptors_behind_the_vtable() {
        let birth = ObjectBirth {
            exotic: ObjectExotic::NativeNamespace,
            null_prototype: false,
            layout: ObjectLayout::Unmarked,
            prototype_mark: PrototypeMark::OnFirstLink,
            descriptors: true,
            typed_array_prototype: false,
            integrity: Integrity::Extensible,
        };
        assert_eq!(object_reserved(birth), 0);
        assert_eq!(object_class_id(birth.exotic), NATIVE_MODULE_CLASS_ID);
    }

    #[test]
    fn a_body_flag_makes_the_instance_bit_redundant() {
        let birth = ClosureBirth {
            pointer_free_captures: false,
            constructable: false,
            body_non_constructor: true,
            integrity: Integrity::Extensible,
        };
        assert_eq!(closure_reserved(birth), 0);
    }
}
