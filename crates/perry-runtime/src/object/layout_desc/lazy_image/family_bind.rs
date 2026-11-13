//! BIND is the held arm of materialize_family. Family ownership is encoded
//! on the cell's header or its descriptor; all temporary collection is on
//! the stack, with no lookup registry or managed allocation.
use super::*;
use crate::object::ObjectHeader;

const PENDING: u32 = crate::codegen_abi::LAYOUT_ROW_PENDING;

/// The cell owning the layout word, using only the emitted representation.
unsafe fn owner(cell: usize, kind: u8, flags: u8) -> Option<usize> {
    match kind {
        crate::gc::GC_TYPE_OBJECT | crate::gc::GC_TYPE_CLOSURE => Some(cell),
        crate::gc_array_type_pat!() if flags & crate::gc::GC_FLAG_SHAPE_SHARED == 0 => {
            let named =
                crate::array::array_property_bag_resolved(cell as *const crate::array::ArrayHeader);
            (!named.is_null()).then_some(named as usize)
        }
        _ => None,
    }
}

unsafe fn holder(cell: usize, kind: u8) -> *mut ObjectHeader {
    match kind {
        crate::gc::GC_TYPE_CLOSURE => (*(cell as *const crate::closure::ClosureHeader)).props,
        crate::gc_array_type_pat!() => {
            crate::array::array_property_bag_resolved(cell as *const crate::array::ArrayHeader)
        }
        crate::gc::GC_TYPE_OBJECT => cell as *mut ObjectHeader,
        _ => std::ptr::null_mut(),
    }
}

unsafe fn facts(cell: usize, kind: u8, flags: u8) -> Option<*const PerryLayoutDesc> {
    let cell = owner(cell, kind, flags)?;
    let word = *(cell as *const u32).add(1);
    let direct = descriptor(word & !PENDING);
    if kind != crate::gc::GC_TYPE_CLOSURE || direct.is_some() {
        return direct;
    }
    let bag = (*(cell as *const crate::closure::ClosureHeader)).props;
    if bag.is_null() {
        return None;
    }
    descriptor((*bag).parent_class_id & !PENDING)
}

unsafe fn family(cell: usize, kind: u8, flags: u8) -> Option<u8> {
    if kind == crate::gc::GC_TYPE_CLOSURE {
        if let Some(identity) = crate::object::BuiltinConstructorIdentity::from_header(
            (*(cell as *const crate::closure::ClosureHeader)).capture_count,
        ) {
            return Some(identity as u8);
        }
    }
    let value = (*facts(cell, kind, flags)?).realm_family;
    (value != 0).then_some(value)
}

fn pointer(bits: u64) -> Option<usize> {
    (bits & !crate::value::POINTER_MASK == crate::value::POINTER_TAG
        && bits & crate::value::POINTER_MASK != 0)
        .then_some((bits & crate::value::POINTER_MASK) as usize)
}

/// Seeding entries must not observe a cell. Read an own data slot by its
/// emitted descriptor ordinal, even while the cell is pending.
unsafe fn own(cell: usize, kind: u8, key: &[u8]) -> Option<usize> {
    let object = holder(cell, kind);
    if object.is_null() {
        return None;
    }
    let word = (*object).parent_class_id;
    let desc = descriptor(word & !PENDING)?;
    for index in 0..(*desc).key_count as usize {
        let property = &*(*desc).keys.add(index);
        if property.attrs & crate::codegen_abi::LAYOUT_KEY_ACCESSOR != 0 {
            continue;
        }
        if super::super::atom_bytes(property.atom).is_some_and(|bytes| bytes == key) {
            let slot = object.cast::<u8>().add(property.off as usize).cast::<u64>();
            return pointer(slot.read());
        }
    }
    None
}

unsafe fn meta(cell: usize, kind: u8) -> *const crate::object::ObjectMeta {
    let object = holder(cell, kind);
    if object.is_null() {
        std::ptr::null()
    } else {
        (*object).meta
    }
}

fn builtin_identity(encoding: u8) -> Option<crate::object::BuiltinConstructorIdentity> {
    if encoding == 0 || encoding as usize > crate::object::GLOBAL_THIS_BUILTIN_CONSTRUCTORS.len() {
        return None;
    }
    crate::object::BuiltinConstructorIdentity::from_header(
        u32::from(encoding) << crate::codegen_abi::CLOSURE_CONSTRUCT_SHIFT,
    )
}

unsafe fn prototype_candidate(encoding: u8, cell: usize, kind: u8, flags: u8) -> bool {
    if let Some(identity) = builtin_identity(encoding) {
        if identity == crate::object::BuiltinConstructorIdentity::Array {
            return crate::gc_type_is_array!(kind) && flags & crate::gc::GC_FLAG_SHAPE_SHARED == 0;
        }
    }
    if kind != crate::gc::GC_TYPE_OBJECT {
        return false;
    }
    let Some(desc) = facts(cell, kind, flags) else {
        return false;
    };
    if (*desc).proto != LAYOUT_PROTO_NULL {
        return true;
    }
    // Object.prototype itself has a null parent, like the private bags, and
    // its existing seal fact distinguishes it from those bags.
    let m = meta(cell, kind);
    if m.is_null() {
        return false;
    }
    builtin_identity(encoding)
        .and_then(|identity| {
            crate::object::builtin_proto::constructor_family(identity.declaration())
        })
        .is_some_and(|family| {
            (*m).flags & crate::object::meta_flags::OBJECT_META_FLAG_SEALED_INTRINSIC != 0
                && ((*m).flags >> crate::object::meta_flags::OBJECT_META_SEALED_FAMILY_SHIFT)
                    & crate::object::meta_flags::OBJECT_META_SEALED_FAMILY_MASK
                    == family as u64
        })
}

#[derive(Default)]
struct Roots {
    constructor: usize,
    prototype: usize,
    iterator: [usize; 8],
    function: [usize; 3],
}

unsafe fn roots(encoding: u8) -> Roots {
    let mut result = Roots::default();
    for header in crate::value::addr_class::realm::cells() {
        let cell = header.cast::<u8>().add(crate::gc::GC_HEADER_SIZE) as usize;
        let kind = (*header).obj_type;
        let flags = (*header).gc_flags;
        if family(cell, kind, flags) != Some(encoding) {
            continue;
        }
        if kind == crate::gc::GC_TYPE_CLOSURE {
            let closure = &*(cell as *const crate::closure::ClosureHeader);
            if crate::object::BuiltinConstructorIdentity::from_header(closure.capture_count)
                .is_some()
            {
                result.constructor = cell;
            } else if encoding == crate::codegen_abi::REALM_FAMILY_TYPED_ARRAY
                && closure.capture_count == 1
                && !closure.info.is_null()
                && (*closure.info).flags & crate::closure::FN_NON_CONSTRUCTOR == 0
            {
                assert!(
                    result.constructor == 0 || result.constructor == cell,
                    "ambiguous TypedArray constructor fact"
                );
                result.constructor = cell;
            }
        }
        let metadata = meta(cell, kind);
        let serial = if metadata.is_null() {
            0
        } else {
            (*metadata).proto_serial
        };
        if encoding == crate::codegen_abi::REALM_FAMILY_ITERATORS {
            let first = perry_abi::intrinsic_serials::INTRINSIC_SERIAL_ITERATOR_PROTOTYPE;
            if kind == crate::gc::GC_TYPE_OBJECT && (first..first + 8).contains(&serial) {
                result.iterator[(serial - first) as usize] = cell;
            }
        } else if encoding == crate::codegen_abi::REALM_FAMILY_FUNCTION_TOWERS {
            let first = perry_abi::intrinsic_serials::INTRINSIC_SERIAL_ASYNC_FUNCTION;
            if kind == crate::gc::GC_TYPE_OBJECT && (first..first + 3).contains(&serial) {
                result.function[(serial - first) as usize] = cell;
            }
        } else if prototype_candidate(encoding, cell, kind, flags) {
            assert!(
                result.prototype == 0 || result.prototype == cell,
                "ambiguous intrinsic prototype fact"
            );
            result.prototype = cell;
        }
    }
    if result.constructor != 0 {
        if let Some(proto) = own(result.constructor, crate::gc::GC_TYPE_CLOSURE, b"prototype") {
            result.prototype = proto;
        }
    }
    if encoding == crate::codegen_abi::REALM_FAMILY_TYPED_ARRAY
        && result.constructor == 0
        && result.prototype != 0
    {
        result.constructor =
            own(result.prototype, crate::gc::GC_TYPE_OBJECT, b"constructor").unwrap_or(0);
    }
    result
}

impl Roots {
    unsafe fn entry(&self, encoding: u8) -> (usize, usize) {
        match encoding {
            crate::codegen_abi::REALM_FAMILY_ITERATORS => (self.iterator[0], 0),
            crate::codegen_abi::REALM_FAMILY_FUNCTION_TOWERS => {
                let proto = self
                    .function
                    .into_iter()
                    .find(|&cell| cell != 0)
                    .unwrap_or(0);
                let ctor = if proto == 0 {
                    0
                } else {
                    own(proto, crate::gc::GC_TYPE_OBJECT, b"constructor").unwrap_or(0)
                };
                (proto, ctor)
            }
            _ => (self.prototype, self.constructor),
        }
    }

    unsafe fn link(&self, encoding: u8) {
        use crate::object::global_this::family_link;
        match encoding {
            crate::codegen_abi::REALM_FAMILY_ITERATORS => family_link::iterators(self.iterator),
            crate::codegen_abi::REALM_FAMILY_FUNCTION_TOWERS => {
                for (index, proto) in self.function.into_iter().enumerate() {
                    if proto == 0 {
                        continue;
                    }
                    let ctor = own(proto, crate::gc::GC_TYPE_OBJECT, b"constructor").unwrap_or(0);
                    let generator =
                        own(proto, crate::gc::GC_TYPE_OBJECT, b"prototype").unwrap_or(0);
                    family_link::function_tower(
                        perry_abi::intrinsic_serials::INTRINSIC_SERIAL_ASYNC_FUNCTION
                            + index as u64,
                        ctor,
                        proto,
                        generator,
                    );
                }
            }
            crate::codegen_abi::REALM_FAMILY_TYPED_ARRAY => {
                family_link::typed_array(self.constructor, self.prototype);
            }
            _ => {
                let identity =
                    builtin_identity(encoding).expect("image row has an undeclared family byte");
                family_link::builtin(
                    identity.declaration(),
                    self.constructor,
                    self.prototype,
                    crate::object::global_this::bound_thread_global(),
                );
            }
        }
    }
}

/// A checked birth must not manufacture a second copy beside any held cell.
#[cfg(any(debug_assertions, test))]
pub(crate) fn holds(encoding: u8) -> bool {
    unsafe {
        crate::value::addr_class::realm::cells().any(|header| {
            let cell = header.cast::<u8>().add(crate::gc::GC_HEADER_SIZE) as usize;
            family(cell, (*header).obj_type, (*header).gc_flags) == Some(encoding)
        })
    }
}

/// Entry seeding is a precondition of the SAME materialize point, including
/// observations before globalThis. It writes only the existing Intrinsics
/// halves, preserving every cell's PENDING word.
pub(crate) fn seed(encoding: u8) {
    if span().is_none() {
        return;
    }
    unsafe {
        let cells = roots(encoding);
        let (proto, ctor) = cells.entry(encoding);
        crate::array::note_family_intrinsic(encoding, proto, ctor);
        if builtin_identity(encoding).is_some_and(|identity| identity.is_typed_array()) {
            // The child names the abstract parent in its emitted facts.
            // Seed its plain roots without observing or binding that family.
            let family = crate::codegen_abi::REALM_FAMILY_TYPED_ARRAY;
            let parent = roots(family);
            let (proto, ctor) = parent.entry(family);
            crate::array::note_family_intrinsic(family, proto, ctor);
        }
    }
}

/// Publish exactly this family's held rows and clear its cells, then use
/// the same external-word body as main's birth.
pub(super) unsafe fn bind(object: *mut ObjectHeader, word: u32) {
    let header = &*object
        .cast::<u8>()
        .sub(crate::gc::GC_HEADER_SIZE)
        .cast::<crate::gc::GcHeader>();
    let encoding = family(object as usize, header.obj_type, header.gc_flags)
        .expect("pending image cell has no family fact");
    for header in crate::value::addr_class::realm::cells() {
        let cell = header.cast::<u8>().add(crate::gc::GC_HEADER_SIZE) as usize;
        if family(cell, (*header).obj_type, (*header).gc_flags) != Some(encoding) {
            continue;
        }
        let Some(owner) = owner(cell, (*header).obj_type, (*header).gc_flags) else {
            continue;
        };
        let slot = (owner as *mut u32).add(1);
        let pending = slot.read();
        if pending & PENDING == 0 {
            continue;
        }
        let bound = pending & !PENDING;
        publish_receiver(bound);
        slot.write(bound);
    }
    assert_eq!(
        (*object).parent_class_id,
        word & !PENDING,
        "family bind did not clear the observed cell"
    );
    roots(encoding).link(encoding);
}
