use super::*;
use crate::codegen_abi::LayoutWord;
use crate::gc::RuntimeHandleScope;
use crate::object::layout_desc::{PerryLayoutKey, StaticAtom};

static ATOM: StaticAtom = StaticAtom {
    hash: 0,
    string: std::ptr::null(),
    kind: crate::codegen_abi::ATOM_KIND_BINDING_SYMBOL,
    _pad: [0; 3],
    index: 0,
};
static KEYS: [PerryLayoutKey; 1] = [PerryLayoutKey {
    atom: &ATOM,
    slot: 0,
    attrs: crate::codegen_abi::LAYOUT_KEY_OPTIONAL,
    _pad: 0,
    off: crate::object::layout_desc::layout_key_off(0 as u32),
}];
static mut SYMBOL: u64 = crate::value::TAG_UNDEFINED;
static mut LAYOUT: PerryLayoutDesc = PerryLayoutDesc {
    image_keys: std::ptr::null_mut(),
    shape_id: 0,
    gc_plan: 0,
    family: 0x464e4201,
    variant: crate::codegen_abi::LAYOUT_VARIANT_FUNCTION_CONSTRUCTOR,
    kind: 0,
    integrity: 0,
    slot_count: 1,
    layout_id: 0,
    rep: 0,
    proto: std::ptr::without_provenance(2),
    keys: KEYS.as_ptr(),
    key_count: 1,
    index_mask: 0,
    index: std::ptr::null(),
    class_id: 0,
    parent_class_id: 0,
    methods: std::ptr::null(),
    gc_header_word: crate::object::layout_desc::layout_gc_header_word(1),
    slots_first: crate::object::layout_desc::layout_slots_first(),
    _tail_pad: [0; 30],
};
unsafe impl Sync for ConstructorOperand {}
static OPERANDS: [ConstructorOperand; 1] = [ConstructorOperand {
    global: std::ptr::addr_of!(SYMBOL),
    capture: 0,
    kind: 0,
    slot: 0,
}];
unsafe impl Sync for ConstructorLayout {}
static TAIL: ConstructorLayout = ConstructorLayout {
    layout: std::ptr::addr_of!(LAYOUT),
    operands: OPERANDS.as_ptr(),
    count: 1,
    _reserved: 0,
};
#[repr(C)]
struct Info {
    base: crate::closure::JsFunctionInfo,
    constructor: ConstructorLayout,
}
unsafe impl Sync for Info {}
extern "C" fn empty(_c: *const ClosureHeader, _this: crate::closure::JsThis) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}
static INFO: Info = Info {
    base: crate::closure::JsFunctionInfo::of(empty as crate::codegen_abi::JsBody0<ClosureHeader>)
        .with_flags(
            crate::codegen_abi::FN_HAS_CONSTRUCTOR_LAYOUT | crate::codegen_abi::FN_COMPILED_BODY,
        ),
    constructor: TAIL,
};

unsafe extern "C" fn read_binding() -> u64 {
    SYMBOL
}
static READER_OPERANDS: [ConstructorOperand; 1] = [ConstructorOperand {
    global: read_binding as *const u64,
    capture: 0,
    kind: 3,
    slot: 0,
}];
static READER_INFO: Info = Info {
    base: crate::closure::JsFunctionInfo::of(empty as crate::codegen_abi::JsBody0<ClosureHeader>)
        .with_flags(
            crate::codegen_abi::FN_HAS_CONSTRUCTOR_LAYOUT | crate::codegen_abi::FN_COMPILED_BODY,
        ),
    constructor: ConstructorLayout {
        operands: READER_OPERANDS.as_ptr(),
        ..TAIL
    },
};
static CAPTURE_OPERANDS: [ConstructorOperand; 1] = [ConstructorOperand {
    global: std::ptr::null(),
    capture: 0,
    kind: 1,
    slot: 0,
}];
static SCOPE_OPERANDS: [ConstructorOperand; 1] = [ConstructorOperand {
    global: std::ptr::null(),
    capture: 0,
    kind: 2,
    slot: 1,
}];
static CAPTURE_INFO: Info = Info {
    base: crate::closure::JsFunctionInfo::of(empty as crate::codegen_abi::JsBody0<ClosureHeader>)
        .with_flags(
            crate::codegen_abi::FN_HAS_CONSTRUCTOR_LAYOUT | crate::codegen_abi::FN_COMPILED_BODY,
        ),
    constructor: ConstructorLayout {
        operands: CAPTURE_OPERANDS.as_ptr(),
        ..TAIL
    },
};
static SCOPE_INFO: Info = Info {
    base: crate::closure::JsFunctionInfo::of(empty as crate::codegen_abi::JsBody0<ClosureHeader>)
        .with_flags(
            crate::codegen_abi::FN_HAS_CONSTRUCTOR_LAYOUT | crate::codegen_abi::FN_COMPILED_BODY,
        ),
    constructor: ConstructorLayout {
        operands: SCOPE_OPERANDS.as_ptr(),
        ..TAIL
    },
};

#[test]
fn family_birth_keeps_holes_and_live_prototype_and_checks_symbol_identity() {
    crate::gc::initialize_runtime_fixture();
    let _lock = crate::gc::global_side_table_test_lock();
    // This test operand is a raw fixture slot; real module operands are roots.
    let _no_move = crate::gc::GcSuppressScope::new();
    let scope = RuntimeHandleScope::new();
    let c = crate::closure::js_closure_alloc(&INFO.base, 0);
    let f = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(c as i64));
    let p = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc_plain(0) as i64,
    ));
    unsafe {
        super::super::proto_validity::mark_object_as_prototype(
            (p.get_nanbox_u64() & crate::value::POINTER_MASK) as usize,
        )
        .unwrap();
        SYMBOL = crate::value::TAG_UNDEFINED;
        assert!(
            allocate(&f, &p).is_none(),
            "an uninitialized operand must decline without an early error"
        );
        let sym = scope.root_nanbox_f64(crate::symbol::js_symbol_new(f64::from_bits(
            crate::value::TAG_UNDEFINED,
        )));
        SYMBOL = sym.get_nanbox_u64();
        let obj = scope.root_raw_mut_ptr(allocate(&f, &p).expect("family path must be live"));
        let first_word = obj.with_const_ptr::<ObjectHeader, _>(|o| (*o).parent_class_id);
        obj.with_const_ptr::<ObjectHeader, _>(|o| {
            // The shared descriptor stays immutable (no per-evaluation word);
            // the object's own header names its layout.
            assert_eq!((*std::ptr::addr_of!(LAYOUT)).layout_id, 0);
            assert!(super::super::shapes::is_layout_word((*o).parent_class_id));
            assert_eq!(
                super::super::shapes::shape_object_kind_for_word(LayoutWord((*o).parent_class_id)),
                Some(super::super::shapes::ShapeObjectKind::Ordinary)
            );
            assert!(
                (*o).meta.is_null(),
                "the shape carries the actual prototype"
            );
            assert_eq!(
                super::super::shapes::object_prototype_word(o),
                p.get_nanbox_u64()
            );
            assert_eq!(
                super::super::object_inline_capacity(o),
                super::super::INLINE_SLOT_FLOOR as u32
            );
            assert_eq!(
                *o.cast::<u8>()
                    .add(std::mem::size_of::<ObjectHeader>())
                    .cast::<u64>(),
                crate::value::TAG_HOLE
            );
        });
        let stored = obj.with_mut_ptr::<ObjectHeader, _>(|o| {
            try_symbol_store(
                std::ptr::addr_of!(LAYOUT),
                0,
                crate::value::js_nanbox_pointer(o as i64),
                sym.get_nanbox_f64(),
                11.0,
            )
        });
        assert!(stored, "the checked fixed-position store must be live");
        obj.with_const_ptr::<ObjectHeader, _>(|o| {
            assert!(super::super::shapes::is_layout_word((*o).parent_class_id));
            assert_eq!(
                *o.cast::<u8>()
                    .add(std::mem::size_of::<ObjectHeader>())
                    .cast::<f64>(),
                11.0
            );
        });
        let q = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::js_object_alloc_plain(0) as i64,
        ));
        super::super::proto_validity::mark_object_as_prototype(
            (q.get_nanbox_u64() & crate::value::POINTER_MASK) as usize,
        )
        .unwrap();
        // The shared descriptor binds nothing: a different prototype is bound
        // at its own birth, to its own layout.
        let other = scope.root_raw_mut_ptr(
            allocate(&f, &q).expect("a different prototype binds its own layout"),
        );
        other.with_const_ptr::<ObjectHeader, _>(|o| {
            assert_ne!((*o).parent_class_id, first_word);
            assert_eq!(
                super::super::shapes::object_prototype_word(o),
                q.get_nanbox_u64()
            );
        });
        let extra = b"later";
        let key = crate::string::js_string_from_bytes(extra.as_ptr(), extra.len() as u32);
        obj.with_mut_ptr::<ObjectHeader, _>(|o| {
            super::super::js_object_set_field_by_name(o, key, 22.0)
        });
        obj.with_const_ptr::<ObjectHeader, _>(|o| {
            assert!(
                !super::super::overflow::has_overflow(o),
                "ordinary growth must not allocate a family overflow"
            );
            assert!(
                (*o).meta.is_null(),
                "late inline growth keeps the prototype in the shape"
            );
            assert_eq!(
                super::super::shapes::object_prototype_word(o),
                p.get_nanbox_u64()
            );
        });

        let reader = crate::closure::js_closure_alloc(&READER_INFO.base, 0);
        let reader = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(reader as i64));
        assert!(
            allocate(&reader, &p).is_some(),
            "immutable binding reader admits the current value"
        );
        // The same immutable body may be closed over another environment.
        let direct = crate::closure::js_closure_alloc(&CAPTURE_INFO.base, 1);
        crate::closure::js_closure_set_capture_bits(direct, 0, sym.get_nanbox_u64());
        let direct = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(direct as i64));
        assert!(
            allocate(&direct, &p).is_some(),
            "current direct capture is admitted"
        );
        let cells = crate::r#box::scope::js_scope_alloc(2, crate::value::TAG_TDZ as i64);
        let scoped = crate::closure::js_closure_alloc(&SCOPE_INFO.base, 1);
        crate::closure::js_closure_set_capture_bits(scoped, 0, cells as u64);
        let scoped = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(scoped as i64));
        assert!(
            allocate(&scoped, &p).is_none(),
            "captured TDZ declines without reading it as JS"
        );
        crate::r#box::scope::js_scope_set(cells, 1, sym.get_nanbox_u64() as i64);
        assert!(
            allocate(&scoped, &p).is_some(),
            "current scope slot is admitted"
        );
        let changed = crate::symbol::js_symbol_new(f64::from_bits(crate::value::TAG_UNDEFINED));
        crate::r#box::scope::js_scope_set(cells, 1, changed.to_bits() as i64);
        let rebound = allocate(&scoped, &p).expect("another binding binds its own layout");
        assert_ne!(
            (*rebound).parent_class_id,
            first_word,
            "another binding cannot reuse this layout"
        );
        SYMBOL =
            crate::symbol::js_symbol_new(f64::from_bits(crate::value::TAG_UNDEFINED)).to_bits();
        let rebound = allocate(&f, &p).expect("a different symbol binds its own layout");
        assert_ne!(
            (*rebound).parent_class_id,
            first_word,
            "a different symbol cannot use the first binding's layout"
        );
        SYMBOL = crate::value::TAG_UNDEFINED;
    }
}

/// #5 (DPA review): per-evaluation function-constructor records are carried by
/// their objects only. A loop of evaluations keeps the record count bounded
/// across full GCs, and the worker seed holds none of the dead ones.
#[test]
fn per_evaluation_records_retire_and_are_not_seeded() {
    crate::gc::initialize_runtime_fixture();
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let scope = RuntimeHandleScope::new();
    let c = crate::closure::js_closure_alloc(&INFO.base, 0);
    let f = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(c as i64));
    let p = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc_plain(0) as i64,
    ));
    let collect = crate::gc::test_collect_synchronous_full_trace;
    unsafe {
        super::super::proto_validity::mark_object_as_prototype(
            (p.get_nanbox_u64() & crate::value::POINTER_MASK) as usize,
        )
        .unwrap();
        let seed_before = super::super::shapes::test_worker_seed_len();
        let mut counts = Vec::new();
        for round in 0..4 {
            for _ in 0..100 {
                SYMBOL = crate::symbol::js_symbol_new(f64::from_bits(crate::value::TAG_UNDEFINED))
                    .to_bits();
                assert!(allocate(&f, &p).is_some());
            }
            SYMBOL = crate::value::TAG_UNDEFINED;
            collect();
            collect();
            counts.push(super::super::shapes::test_record_count());
            assert!(
                round == 0 || counts[round] <= counts[0] + 8,
                "records grew: {counts:?}"
            );
        }
        assert_eq!(
            super::super::shapes::test_worker_seed_len(),
            seed_before,
            "a dead per-evaluation record was seeded"
        );
    }
}
