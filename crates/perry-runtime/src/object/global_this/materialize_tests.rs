//! S2 structural fixtures: external words after a real main birth must
//! agree with first-observation BIND of the same family's structural cells.
#![cfg(target_pointer_width = "64")]
use super::*;
use crate::codegen_abi::{
    LayoutRow, LayoutWord, PerryLayoutImageV1, PerryProgramTableV1, ShapeObjectKind,
};
use crate::gc::GcHeader;
use crate::object::layout_desc::{PerryLayoutDesc, PerryLayoutKey, PerryProtoFamily};

const FIRST: u32 = 16384;
const ROWS: usize = 10;
const PENDING: u32 = crate::codegen_abi::LAYOUT_ROW_PENDING;
const FLAGS: u8 = crate::value::addr_class::realm::IMAGE_CELL_FLAGS;

#[repr(C, align(8))]
struct ObjectCell {
    gc: GcHeader,
    object: ObjectHeader,
    slots: [u64; 2],
}
#[repr(C, align(8))]
struct ClosureCell {
    gc: GcHeader,
    closure: crate::closure::ClosureHeader,
    capture: [u64; 1],
}
#[repr(C, align(8))]
struct MetaCell {
    gc: GcHeader,
    meta: crate::object::ObjectMeta,
}
#[repr(C, align(8))]
struct ArrayCell {
    gc: GcHeader,
    array: crate::array::ArrayHeader,
}
#[repr(C, align(8))]
struct ListCell {
    gc: GcHeader,
    array: crate::array::ArrayHeader,
    slots: [u64; 2],
}

const fn gc(kind: u8, size: usize, flags: u8) -> GcHeader {
    GcHeader {
        obj_type: kind,
        gc_flags: flags,
        _reserved: 0,
        size: size as u32,
    }
}
const fn object_cell() -> ObjectCell {
    ObjectCell {
        gc: gc(
            crate::gc::GC_TYPE_OBJECT,
            std::mem::size_of::<ObjectCell>(),
            FLAGS,
        ),
        object: ObjectHeader {
            class_id: 0,
            parent_class_id: 0,
            meta: std::ptr::null_mut(),
        },
        slots: [crate::value::TAG_UNDEFINED; 2],
    }
}
const fn closure_cell() -> ClosureCell {
    ClosureCell {
        gc: gc(
            crate::gc::GC_TYPE_CLOSURE,
            std::mem::size_of::<ClosureCell>(),
            FLAGS,
        ),
        closure: crate::closure::ClosureHeader {
            capture_count: 0,
            shape_id: LayoutWord(0),
            info: &crate::value::addr_class::realm::perry_realm_fixture_info,
            props: std::ptr::null_mut(),
        },
        capture: [0],
    }
}
const fn meta_cell() -> MetaCell {
    MetaCell {
        gc: gc(
            crate::gc::GC_TYPE_OBJECT_META,
            std::mem::size_of::<MetaCell>(),
            FLAGS,
        ),
        meta: unsafe { std::mem::zeroed() },
    }
}
const fn array_cell() -> ArrayCell {
    ArrayCell {
        gc: gc(
            crate::gc::GC_TYPE_ARRAY,
            std::mem::size_of::<ArrayCell>(),
            FLAGS,
        ),
        array: crate::array::ArrayHeader {
            length: 0,
            capacity: 0,
            named: std::ptr::null_mut(),
            ek_prefix: 0,
            ek_kind: 0,
        },
    }
}
const fn list_cell() -> ListCell {
    ListCell {
        gc: gc(
            crate::gc::GC_TYPE_ARRAY,
            std::mem::size_of::<ListCell>(),
            FLAGS | crate::gc::GC_FLAG_SHAPE_SHARED,
        ),
        array: crate::array::ArrayHeader {
            length: 0,
            capacity: 2,
            named: std::ptr::null_mut(),
            ek_prefix: 0,
            ek_kind: 0,
        },
        slots: [0; 2],
    }
}
#[repr(C, align(8))]
struct ImageFixture {
    objects: [ObjectCell; 16],
    closures: [ClosureCell; 4],
    metadata: [MetaCell; 16],
    arrays: [ArrayCell; 1],
    lists: [ListCell; 4],
}
const fn fixture() -> ImageFixture {
    ImageFixture {
        objects: [const { object_cell() }; 16],
        closures: [const { closure_cell() }; 4],
        metadata: [const { meta_cell() }; 16],
        arrays: [const { array_cell() }; 1],
        lists: [const { list_cell() }; 4],
    }
}
#[used]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_realm")]
#[cfg_attr(target_os = "windows", link_section = ".perryrealm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_realm"
)]
static mut IMAGE: ImageFixture = fixture();
static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[repr(C, align(8))]
struct PString {
    gc: GcHeader,
    string: crate::StringHeader,
    bytes: [u8; 4],
    hash: u64,
}
#[used]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static P_STRING: PString = PString {
    gc: GcHeader {
        obj_type: crate::gc::GC_TYPE_STRING,
        gc_flags: FLAGS | crate::gc::GC_FLAG_INTERNED,
        _reserved: crate::string::STRING_IMAGE_MARKER,
        size: crate::string::image_string_gc_size(1) as u32,
    },
    string: crate::StringHeader {
        utf16_len: 1,
        byte_len: 1,
        capacity: 1,
        refcount: 0,
        flags: crate::string::STRING_FLAG_WTF8_VALIDATED | crate::string::IMAGE_ATOM_FLAGS,
    },
    bytes: *b"p\0\0\0",
    hash: crate::object::runtime_layouts::atom_hash(b"p"),
};
static mut P_ATOM: crate::object::layout_desc::StaticAtom =
    crate::object::layout_desc::StaticAtom {
        hash: 0,
        string: std::ptr::addr_of!(P_STRING.string),
        kind: crate::codegen_abi::ATOM_KIND_STRING,
        _pad: [0; 3],
        index: 0,
    };
const fn key(atom: *const crate::object::layout_desc::StaticAtom, slot: u32) -> PerryLayoutKey {
    PerryLayoutKey {
        atom,
        slot,
        attrs: 0,
        _pad: 0,
        off: crate::object::layout_desc::layout_key_off(slot),
    }
}
static PROTOTYPE: [PerryLayoutKey; 1] =
    [key(&crate::object::runtime_layouts::BP_ATOM_PROTOTYPE, 0)];
static CONSTRUCTOR: [PerryLayoutKey; 1] =
    [key(&crate::object::runtime_layouts::BP_ATOM_CONSTRUCTOR, 0)];
static TOWER: [PerryLayoutKey; 2] = [
    key(&crate::object::runtime_layouts::BP_ATOM_CONSTRUCTOR, 0),
    key(&crate::object::runtime_layouts::BP_ATOM_PROTOTYPE, 1),
];
static STATE: [PerryLayoutKey; 1] = [key(std::ptr::addr_of!(P_ATOM), 0)];
static mut INDEX: [[u32; 4]; 4] = [[u32::MAX; 4]; 4];
static mut FUNCTION: PerryProtoFamily = PerryProtoFamily {
    serial: crate::closure::shape::INTRINSIC_SERIAL_FUNCTION,
    image_prototype: std::ptr::null(),
};
const fn descriptor() -> PerryLayoutDesc {
    PerryLayoutDesc {
        family: 0,
        variant: 0,
        kind: ShapeObjectKind::Ordinary as u8,
        integrity: 0,
        slot_count: 0,
        layout_id: 0,
        rep: 0,
        proto: crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
        keys: std::ptr::null(),
        key_count: 0,
        index_mask: 0,
        index: std::ptr::null(),
        class_id: 0,
        parent_class_id: 0,
        methods: std::ptr::null(),
        shape_id: 0,
        gc_plan: 0,
        image_keys: std::ptr::null_mut(),
        gc_header_word: crate::object::layout_desc::layout_gc_header_word(0),
        realm_family: 0,
        _tail_pad: [0; 31],
    }
}
static mut FACTS: [PerryLayoutDesc; ROWS] = [const { descriptor() }; ROWS];
unsafe fn row_word(index: usize) -> u32 {
    FACTS[index].layout_id
}
unsafe fn object(index: usize, row: usize) -> *mut ObjectHeader {
    let p = std::ptr::addr_of_mut!(IMAGE.objects[index].object);
    (*p).parent_class_id = row_word(row) | PENDING;
    (*p).meta = std::ptr::addr_of_mut!(IMAGE.metadata[index].meta);
    p
}
unsafe fn closure(index: usize, bag: usize, row: usize) -> *mut crate::closure::ClosureHeader {
    let p = std::ptr::addr_of_mut!(IMAGE.closures[index].closure);
    (*p).props = object(bag, 1);
    (*p).shape_id = LayoutWord(row_word(row) | PENDING);
    p
}
fn pointer(addr: usize) -> u64 {
    crate::value::JSValue::pointer(addr as *mut u8).bits()
}

unsafe fn prepare_rows(encoding: u8) {
    IMAGE = fixture();
    FACTS = [const { descriptor() }; ROWS];
    FUNCTION.image_prototype = std::ptr::null();
    P_ATOM.hash = crate::object::key_bytes_hash(b"p".as_ptr(), 1);
    let key_sets: [&[PerryLayoutKey]; 4] = [&PROTOTYPE, &CONSTRUCTOR, &TOWER, &STATE];
    for (list, keys) in key_sets.into_iter().enumerate() {
        INDEX[list] = [u32::MAX; 4];
        IMAGE.lists[list].array.length = keys.len() as u32;
        for (ordinal, key) in keys.iter().enumerate() {
            IMAGE.lists[list].slots[ordinal] = crate::value::STRING_TAG | (*key.atom).string as u64;
            let mut index = (*key.atom).hash as usize & 3;
            while INDEX[list][index] != u32::MAX {
                index = (index + 1) & 3;
            }
            INDEX[list][index] = ordinal as u32;
        }
    }
    for i in 0..ROWS {
        FACTS[i].realm_family = encoding;
        FACTS[i].proto = crate::object::layout_desc::LAYOUT_PROTO_NULL;
        FACTS[i].layout_id = LayoutWord::new(LayoutRow(FIRST + i as u32), 0, 2)
            .unwrap()
            .0;
    }
    for i in [2usize, 7] {
        FACTS[i].kind = ShapeObjectKind::Function as u8;
        FACTS[i].proto = std::ptr::addr_of!(FUNCTION);
        FACTS[i].layout_id = LayoutWord::new(LayoutRow(FIRST + i as u32), 2, 1)
            .unwrap()
            .0;
    }
    for (row, list) in [(1usize, 0usize), (3, 2), (4, 1), (5, 3), (9, 0)] {
        let keys = key_sets[list];
        FACTS[row].keys = keys.as_ptr();
        FACTS[row].slot_count = keys.len() as u32;
        FACTS[row].key_count = keys.len() as u32;
        FACTS[row].gc_header_word =
            crate::object::layout_desc::layout_gc_header_word(keys.len() as u32);
        FACTS[row].index = std::ptr::addr_of!(INDEX[list]).cast();
        FACTS[row].index_mask = 3;
        FACTS[row].image_keys = std::ptr::addr_of_mut!(IMAGE.lists[list].array);
    }
    let other = if encoding == BuiltinConstructorIdentity::Date as u8 {
        BuiltinConstructorIdentity::Map
    } else {
        BuiltinConstructorIdentity::Date
    };
    FACTS[6].realm_family = other as u8;
    FACTS[6].proto = crate::object::layout_desc::LAYOUT_PROTO_OBJECT;
    object(15, 6);
}
unsafe fn enter_image() {
    crate::gc::image_cells::hold_image_for_tests(true);
    crate::object::class_image::enter_current_thread_image();
    let program = Box::leak(Box::new(PerryProgramTableV1 {
        lazy_first: FIRST,
        lazy_count: ROWS as u32,
        lazy_layouts: std::ptr::addr_of!(FACTS).cast(),
        touch_pending: Some(crate::object::layout_desc::js_realm_touch_pending),
        ..PerryProgramTableV1::EMPTY
    }));
    crate::object::class_image::bind_layout_image(PerryLayoutImageV1 {
        static_rows: FIRST + ROWS as u32,
        family_count: FIRST - crate::codegen_abi::NATIVE_LAYOUT_ROWS,
        program,
        ..PerryLayoutImageV1::RUNTIME
    });
    crate::test_support::bootstrap_test_thread();
    crate::gc::ensure_gc_initialized();
}
unsafe fn cleanup() {
    // Leave no pending words or pointer-bearing image fixtures for later tests.
    IMAGE = fixture();
    crate::gc::image_cells::hold_image_for_tests(false);
}

#[derive(Debug, PartialEq)]
struct External {
    words: Vec<u64>,
}
unsafe fn external(identity: BuiltinConstructorIdentity, proto: usize, ctor: usize) -> External {
    let decl = identity.declaration();
    let holder = if identity == BuiltinConstructorIdentity::Array {
        crate::array::array_property_bag_resolved(proto as *const crate::array::ArrayHeader)
            as usize
    } else {
        proto
    };
    let normal = |address: usize| -> u64 {
        if address == 0 {
            0
        } else if address == proto {
            1
        } else if address == ctor {
            2
        } else if address == holder {
            3
        } else {
            panic!("external word points outside the declared family: {address:x}")
        }
    };
    let normal_bits = |bits: u64| {
        if bits == 0 {
            0
        } else {
            normal((bits & crate::value::POINTER_MASK) as usize)
        }
    };
    let (p, c) = crate::array::realm_intrinsic_cells(identity);
    assert_eq!((p, c), (proto, ctor), "plain Intrinsics halves");
    let mut words = vec![normal(p), normal(c)];
    if let Some(family) = crate::object::builtin_proto::constructor_family(decl) {
        let addr = crate::agent_ptrs::intrinsic_proto(family as usize) as usize;
        assert_eq!(addr, holder, "BuiltinProto agent root");
        words.push(normal(addr));
    }
    for serial in [
        decl.prototype_class
            .map(|id| crate::object::shapes::PROTO_ID_CLASS | u64::from(id)),
        decl.prototype_serial,
        decl.named_serial,
        decl.sealed_serial,
    ]
    .into_iter()
    .flatten()
    {
        let expected = if decl.named_serial == Some(serial) {
            holder
        } else {
            proto
        };
        let bits = crate::object::shapes::identity_prototype_word(serial);
        assert_eq!(bits, pointer(expected), "identity serial {serial}");
        words.push(normal_bits(bits));
    }
    use std::sync::atomic::Ordering;
    match identity {
        BuiltinConstructorIdentity::Object => {
            let p = OBJECT_INTRINSIC_PROTO_PTR.load(Ordering::Acquire) as usize;
            let c = OBJECT_INTRINSIC_PTR.load(Ordering::Acquire) as usize;
            assert_eq!((p, c), (proto, ctor));
            assert_eq!(*crate::array::object_prototype_cell(), proto);
            words.extend([
                normal(p),
                normal(c),
                normal(*crate::array::object_prototype_cell()),
            ]);
        }
        BuiltinConstructorIdentity::Function => {
            let p = crate::closure::shape::FUNCTION_PROTOTYPE_PTR.load(Ordering::Acquire) as usize;
            assert_eq!(p, proto);
            words.push(normal(p));
        }
        BuiltinConstructorIdentity::Error => {
            let c = ERROR_CONSTRUCTOR_PTR.with(|c| c.get());
            assert_eq!(c, ctor);
            words.push(normal(c));
        }
        _ => {}
    }
    External { words }
}
struct Birth {
    identity: BuiltinConstructorIdentity,
    flags: u64,
    serial: u64,
    class: u32,
    kind: u8,
    external: External,
}
fn birth(name: &'static str) -> Birth {
    std::thread::spawn(move || unsafe {
        crate::test_support::bootstrap_test_thread();
        js_get_global_this();
        let identity = GLOBAL_THIS_BUILTIN_CONSTRUCTORS
            .iter()
            .find(|d| d.name == name)
            .unwrap()
            .identity;
        let proto = crate::array::intrinsic_prototype_addr(identity);
        let ctor = crate::array::intrinsic_constructor_addr(identity);
        assert_ne!(proto, 0);
        assert_ne!(ctor, 0);
        let holder = if identity == BuiltinConstructorIdentity::Array {
            crate::array::array_property_bag_resolved(proto as *const crate::array::ArrayHeader)
        } else {
            proto as *mut ObjectHeader
        };
        let meta = &*(*holder).meta;
        let kind = crate::object::shapes::object_shape_descriptor(holder)
            .unwrap()
            .object_kind
            .code() as u8;
        Birth {
            identity,
            flags: meta.flags,
            serial: meta.proto_serial,
            class: (*holder).class_id,
            kind,
            external: external(identity, proto, ctor),
        }
    })
    .join()
    .unwrap()
}
unsafe fn builtin_fixture(source: &Birth) -> (usize, usize) {
    prepare_rows(source.identity as u8);
    FACTS[0].kind = source.kind;
    FACTS[0].proto = if source.identity == BuiltinConstructorIdentity::Object {
        crate::object::layout_desc::LAYOUT_PROTO_NULL
    } else {
        crate::object::layout_desc::LAYOUT_PROTO_OBJECT
    };
    // The band/prototype kind is part of the emitted layout word.
    FACTS[0].layout_id = LayoutWord::new(
        LayoutRow(FIRST),
        0,
        if source.identity == BuiltinConstructorIdentity::Object {
            2
        } else {
            0
        },
    )
    .unwrap()
    .0;
    let holder = object(0, 0);
    (*holder).class_id = source.class;
    IMAGE.metadata[0].meta.flags = source.flags;
    IMAGE.metadata[0].meta.proto_serial = source.serial;
    let proto = if source.identity == BuiltinConstructorIdentity::Array {
        IMAGE.arrays[0].array.named = holder;
        std::ptr::addr_of_mut!(IMAGE.arrays[0].array) as usize
    } else {
        holder as usize
    };
    let ctor = closure(0, 1, 2);
    (*ctor).info = source.identity.declaration().info;
    (*ctor).capture_count = (source.identity as u32) << crate::codegen_abi::CLOSURE_CONSTRUCT_SHIFT;
    IMAGE.objects[1].slots[0] = pointer(proto);
    (proto, ctor as usize)
}

#[test]
fn materialize_bind_matches_birth_for_every_builtin_proto_family() {
    let _lock = LOCK.lock().unwrap();
    for (_, name) in perry_abi::BuiltinProto::ALL {
        let source = birth(name);
        std::thread::spawn(move || unsafe {
            let (proto, ctor) = builtin_fixture(&source);
            enter_image();
            assert_eq!(crate::array::realm_intrinsic_cells(source.identity), (0, 0));
            let _no_alloc = crate::object::layout_desc::NoGcAlloc::enter();
            crate::object::materialize_family(MaterializeFamily::Builtin(source.identity));
            assert_eq!(
                external(source.identity, proto, ctor),
                source.external,
                "{name}"
            );
            assert_eq!(IMAGE.objects[0].object.parent_class_id & PENDING, 0);
            assert_ne!(IMAGE.objects[15].object.parent_class_id & PENDING, 0);
            drop(_no_alloc);
            cleanup();
        })
        .join()
        .unwrap();
    }
}

#[test]
fn materialize_new_map_before_any_touch_uses_image_prototype() {
    let _lock = LOCK.lock().unwrap();
    let source = birth("Map");
    std::thread::spawn(move || unsafe {
        let (proto, ctor) = builtin_fixture(&source);
        enter_image();
        assert!(!global_this_is_materialized());
        assert_eq!(crate::array::realm_intrinsic_cells(source.identity), (0, 0));
        let map = crate::map::js_map_alloc(0);
        let actual = crate::object::js_object_get_prototype_of(f64::from_bits(
            crate::value::JSValue::pointer(map.cast()).bits(),
        ));
        assert_eq!(actual.to_bits(), pointer(proto));
        assert_eq!(external(source.identity, proto, ctor), source.external);
        assert!(
            !global_this_is_materialized(),
            "image family observation must precede realm birth"
        );
        cleanup();
    })
    .join()
    .unwrap();
}

#[test]
fn materialize_prototype_only_family_never_builds_missing_constructor() {
    let _lock = LOCK.lock().unwrap();
    let source = birth("Object");
    std::thread::spawn(move || unsafe {
        let (proto, _) = builtin_fixture(&source);
        IMAGE.closures[0] = closure_cell();
        IMAGE.objects[1] = object_cell();
        enter_image();
        let no_alloc = crate::object::layout_desc::NoGcAlloc::enter();
        let (ctor, actual) = super::object_intrinsic::ensure_object_intrinsics();
        assert!(ctor.is_null());
        assert_eq!(actual as usize, proto);
        assert_eq!(
            crate::array::realm_intrinsic_cells(source.identity),
            (proto, 0)
        );
        assert_eq!(crate::array::intrinsic_constructor_addr(source.identity), 0);
        assert!(!global_this_is_materialized());
        drop(no_alloc);
        cleanup();
    })
    .join()
    .unwrap();
}

fn tower_words() -> [usize; 8] {
    use std::sync::atomic::Ordering;
    [
        ASYNC_FUNCTION_INTRINSIC_PTR.load(Ordering::Acquire) as usize,
        ASYNC_FUNCTION_INTRINSIC_PROTO_PTR.load(Ordering::Acquire) as usize,
        GENERATOR_FUNCTION_INTRINSIC_PTR.load(Ordering::Acquire) as usize,
        GENERATOR_INTRINSIC_PROTO_PTR.load(Ordering::Acquire) as usize,
        GENERATOR_PROTOTYPE_PTR.load(Ordering::Acquire) as usize,
        ASYNC_GENERATOR_FUNCTION_INTRINSIC_PTR.load(Ordering::Acquire) as usize,
        ASYNC_GENERATOR_INTRINSIC_PROTO_PTR.load(Ordering::Acquire) as usize,
        ASYNC_GENERATOR_PROTOTYPE_PTR.load(Ordering::Acquire) as usize,
    ]
}
#[test]
fn materialize_generator_before_tower_touch_links_all_eight_birth_roots() {
    let _lock = LOCK.lock().unwrap();
    std::thread::spawn(|| {
        crate::test_support::bootstrap_test_thread();
        super::generator::ensure_generator_intrinsics();
        let words = tower_words();
        assert!(words.into_iter().all(|word| word != 0));
        assert_eq!(
            words
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            8
        );
    })
    .join()
    .unwrap();
    std::thread::spawn(|| unsafe {
        prepare_rows(crate::codegen_abi::REALM_FAMILY_FUNCTION_TOWERS);
        let mut constructors = [0; 3];
        let mut prototypes = [0; 3];
        let mut generators = [0; 2];
        for i in 0..2 {
            generators[i] = object(3 + i, 0) as usize;
        }
        for i in 0..3 {
            let p = object(i, if i == 0 { 4 } else { 3 });
            (*(*p).meta).proto_serial =
                perry_abi::intrinsic_serials::INTRINSIC_SERIAL_ASYNC_FUNCTION + i as u64;
            let c = closure(i, 5 + i, 2);
            IMAGE.objects[5 + i].slots[0] = pointer(p as usize);
            IMAGE.objects[i].slots[0] = pointer(c as usize);
            if i > 0 {
                IMAGE.objects[i].slots[1] = pointer(generators[i - 1]);
            }
            constructors[i] = c as usize;
            prototypes[i] = p as usize;
        }
        enter_image();
        assert_eq!(tower_words(), [0; 8]);
        let no_alloc = crate::object::layout_desc::NoGcAlloc::enter();
        super::generator::ensure_generator_intrinsics();
        assert_eq!(
            tower_words(),
            [
                constructors[0],
                prototypes[0],
                constructors[1],
                prototypes[1],
                generators[0],
                constructors[2],
                prototypes[2],
                generators[1],
            ]
        );
        for i in 0..3 {
            assert_eq!(
                crate::object::shapes::identity_prototype_word(
                    perry_abi::intrinsic_serials::INTRINSIC_SERIAL_ASYNC_FUNCTION + i as u64
                ),
                pointer(prototypes[i])
            );
        }
        assert!(!global_this_is_materialized());
        assert_ne!(IMAGE.objects[15].object.parent_class_id & PENDING, 0);
        drop(no_alloc);
        cleanup();
    })
    .join()
    .unwrap();
}

#[test]
fn materialize_typed_child_pending_dictionary_constructor_and_image_parent() {
    let _lock = LOCK.lock().unwrap();
    let source = birth("Uint8Array");
    std::thread::spawn(move || unsafe {
        use std::sync::atomic::Ordering;
        let (proto, ctor) = builtin_fixture(&source);
        for i in [7usize, 8, 9] {
            FACTS[i].realm_family = crate::codegen_abi::REALM_FAMILY_TYPED_ARRAY;
        }
        FACTS[8].proto = crate::object::layout_desc::LAYOUT_PROTO_OBJECT;
        FACTS[8].layout_id = LayoutWord::new(LayoutRow(FIRST + 8), 0, 0).unwrap().0;
        let parent_proto = object(3, 8) as usize;
        let parent_ctor = closure(1, 4, 7);
        (*parent_ctor).capture_count = 1;
        (*parent_ctor).info = source.identity.declaration().info;
        IMAGE.objects[4].object.parent_class_id = row_word(9) | PENDING;
        IMAGE.objects[4].slots[0] = pointer(parent_proto);
        IMAGE.closures[1].capture[0] = pointer(parent_proto);
        let state = object(2, 5);
        IMAGE.objects[2].slots[0] = pointer(parent_ctor as usize);
        IMAGE.metadata[1].meta.expando = pointer(state as usize);
        IMAGE.metadata[0].meta.prototype = pointer(parent_proto);
        // The composed emitter keeps native dictionary headers; their bag
        // owns the lazy own-key row and the pending fact.
        (*(ctor as *mut crate::closure::ClosureHeader)).shape_id =
            LayoutWord(crate::codegen_abi::FUNCTION_DICTIONARY_LAYOUT_WORD);
        enter_image();
        let no_alloc = crate::object::layout_desc::NoGcAlloc::enter();
        crate::object::layout_desc::seed_image_family(source.identity as u8);
        assert_eq!(
            crate::array::intrinsic_constructor_addr_if_bound(source.identity),
            0,
            "a pending dictionary bag keeps its constructor unbound"
        );
        assert_eq!(
            crate::array::intrinsic_constructor_addr(source.identity),
            ctor
        );
        assert_eq!(
            crate::array::realm_intrinsic_cells(source.identity),
            (proto, ctor)
        );
        assert_eq!(
            TYPED_ARRAY_INTRINSIC_PTR.load(Ordering::Acquire) as usize,
            parent_ctor as usize
        );
        assert_eq!(
            TYPED_ARRAY_INTRINSIC_PROTO_PTR.load(Ordering::Acquire) as usize,
            parent_proto
        );
        assert_ne!(
            IMAGE.objects[3].object.parent_class_id & PENDING,
            0,
            "child linking reads parent cells without observing the parent family"
        );
        let (actual_ctor, actual_proto) = super::typed_array::ensure_typed_array_intrinsic();
        assert_eq!(
            (actual_ctor as usize, actual_proto as usize),
            (parent_ctor as usize, parent_proto)
        );
        assert_eq!(IMAGE.objects[3].object.parent_class_id & PENDING, 0);
        assert_eq!((*parent_ctor).shape_id.0 & PENDING, 0);
        drop(no_alloc);
        cleanup();
    })
    .join()
    .unwrap();
}
fn iterator_words() -> [usize; 8] {
    use crate::object::iterator_prototypes as roots;
    use std::sync::atomic::Ordering;
    [
        &roots::ITERATOR_PROTOTYPE_PTR,
        &roots::ARRAY_ITERATOR_PROTOTYPE_PTR,
        &roots::MAP_ITERATOR_PROTOTYPE_PTR,
        &roots::SET_ITERATOR_PROTOTYPE_PTR,
        &roots::STRING_ITERATOR_PROTOTYPE_PTR,
        &roots::REGEXP_STRING_ITERATOR_PROTOTYPE_PTR,
        &roots::ITERATOR_HELPER_PROTOTYPE_PTR,
        &roots::WRAP_FOR_VALID_ITERATOR_PROTOTYPE_PTR,
    ]
    .map(|slot| slot.load(Ordering::Acquire) as usize)
}
#[test]
fn materialize_iterator_group_matches_all_birth_identity_words() {
    let _lock = LOCK.lock().unwrap();
    std::thread::spawn(|| unsafe {
        crate::test_support::bootstrap_test_thread();
        crate::object::iterator_prototypes::ensure_iterator_prototypes();
        let words = iterator_words();
        assert!(words.into_iter().all(|word| word != 0));
        assert_eq!(
            words
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            8
        );
        for (i, word) in words.into_iter().enumerate() {
            let meta = (*(word as *const ObjectHeader)).meta;
            assert_eq!(
                (*meta).proto_serial,
                perry_abi::intrinsic_serials::INTRINSIC_SERIAL_ITERATOR_PROTOTYPE + i as u64
            );
            assert_eq!(
                crate::object::shapes::identity_prototype_word((*meta).proto_serial),
                pointer(word)
            );
        }
    })
    .join()
    .unwrap();
    std::thread::spawn(|| unsafe {
        prepare_rows(crate::codegen_abi::REALM_FAMILY_ITERATORS);
        let cells = std::array::from_fn(|i| {
            let object = object(i, 0);
            (*(*object).meta).proto_serial =
                perry_abi::intrinsic_serials::INTRINSIC_SERIAL_ITERATOR_PROTOTYPE + i as u64;
            object as usize
        });
        enter_image();
        assert_eq!(iterator_words(), [0; 8]);
        let no_alloc = crate::object::layout_desc::NoGcAlloc::enter();
        crate::object::iterator_prototypes::ensure_iterator_prototypes();
        assert_eq!(iterator_words(), cells);
        for (i, cell) in cells.into_iter().enumerate() {
            assert_eq!(
                crate::object::shapes::identity_prototype_word(
                    perry_abi::intrinsic_serials::INTRINSIC_SERIAL_ITERATOR_PROTOTYPE + i as u64
                ),
                pointer(cell)
            );
        }
        assert!(!global_this_is_materialized());
        drop(no_alloc);
        cleanup();
    })
    .join()
    .unwrap();
}

#[test]
fn materialize_pending_constructor_and_shared_iterator_prototype_bind_both_halves() {
    let _lock = LOCK.lock().unwrap();
    let source = birth("Iterator");
    std::thread::spawn(move || unsafe {
        let (proto, ctor) = builtin_fixture(&source);
        FACTS[0].realm_family = crate::codegen_abi::REALM_FAMILY_ITERATORS;
        IMAGE.metadata[0].meta.proto_serial =
            perry_abi::intrinsic_serials::INTRINSIC_SERIAL_ITERATOR_PROTOTYPE;
        let mut cells = [0; 8];
        cells[0] = proto;
        for i in 1..8 {
            let p = object(i + 1, 0);
            (*(*p).meta).proto_serial =
                perry_abi::intrinsic_serials::INTRINSIC_SERIAL_ITERATOR_PROTOTYPE + i as u64;
            cells[i] = p as usize;
        }
        enter_image();
        let no_alloc = crate::object::layout_desc::NoGcAlloc::enter();
        materialize_family(MaterializeFamily::Builtin(source.identity));
        assert_eq!(iterator_words(), cells);
        assert_eq!(
            (*(ctor as *const crate::closure::ClosureHeader)).shape_id.0 & PENDING,
            0,
            "the shared prototype's family cannot leave the requested constructor pending"
        );
        assert_eq!(
            crate::array::realm_intrinsic_cells(source.identity),
            (proto, ctor)
        );
        assert_eq!(IMAGE.objects[1].object.parent_class_id & PENDING, 0);
        drop(no_alloc);
        cleanup();
    })
    .join()
    .unwrap();
}

#[cfg(feature = "regex-engine")]
#[test]
fn pending_family_link_global_private_slot_has_one_declared_regexp_key() {
    unsafe {
        let desc = &*crate::object::runtime_layouts::GLOBAL_OBJECT::desc();
        let keys = std::slice::from_raw_parts(desc.keys, desc.key_count as usize);
        let private: Vec<_> = keys
            .iter()
            .filter(|key| crate::object::key_attrs::entry_is_private(key.attrs))
            .collect();
        assert_eq!(private.len(), 1);
        assert!(std::ptr::eq(
            private[0].atom,
            &crate::object::runtime_layouts::intrinsic_private_atoms::REGEXP_PROTOTYPE
        ));
    }
}
