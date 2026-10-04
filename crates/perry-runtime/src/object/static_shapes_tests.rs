//! The static-id seeds (design step 4, §7.3): the mint adopts a requested
//! static id only on a by-facts miss, and a refusal (the id outside its band,
//! or already naming other facts) aborts: ids are assigned by content.
use super::*;
use crate::object::shapes::{is_shape_id, SHAPE_ID_BASE, STATIC_SHAPE_ID_END};

fn seed(requested: u32, names: &[&str]) -> u32 {
    seed_with_rep(requested, names, 0)
}

fn seed_with_rep(requested: u32, names: &[&str], rep: u64) -> u32 {
    let packed: Vec<u8> = names
        .iter()
        .flat_map(|n| n.bytes().chain(std::iter::once(0)))
        .collect();
    js_shape_seed_plain(
        requested,
        packed.as_ptr(),
        packed.len() as u32,
        names.len() as u32,
        names.len() as u32,
        rep,
    )
}

// Literal fixtures use the same premarking allocation entry as production
// plain records. The legacy shape-cache allocator alone is classless but
// OrdinaryUnmarked, so it cannot establish a ConstFn promotion premise.
fn alloc_constfn_plain_fixture(names: &[&[u8]]) -> *mut crate::object::ObjectHeader {
    let keys = unsafe { canonical_keys_for_names(names) };
    let obj = crate::object::alloc_plain::alloc_plain_record_with_keys(names.len() as u32, keys);
    let id = unsafe { shapes::object_shape_stamp(obj) };
    let birth = shapes::shape_descriptor_by_id(id).expect("plain fixture birth descriptor");
    assert_eq!(birth.object_kind, shapes::ShapeObjectKind::Ordinary);
    assert_eq!(birth.proto_id, 0);
    assert_eq!(birth.logical_key_count, names.len() as u32);
    assert_eq!(birth.live_inline_slot_count, names.len() as u32);
    assert_eq!(birth.rep, crate::object::field_rep::REP_ANY);
    assert_eq!(birth.semantic_generation, 0);
    assert_eq!(birth.hole_count, 0);
    assert_eq!(birth.summary, 0);
    assert_eq!(birth.special_constfn_mask, 0);
    obj
}

extern "C" fn seeded_constfn_body_a(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    11.0
}

extern "C" fn seeded_constfn_body_b(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    22.0
}

/// The seed and module-init class mint must use exactly the same body-aware
/// interner. Production literal finalization uses the same final facts.
#[test]
fn constfn_static_seed_and_module_init_mint_have_identical_facts() {
    use crate::object::field_rep::{with_slot_rep, REP_SPECIAL};
    let _lock = crate::gc::global_side_table_test_lock();
    let info_a = crate::fn_info!(seeded_constfn_body_a, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE));
    let info_b = crate::fn_info!(seeded_constfn_body_b, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE));
    let a = [ConstFnStaticEntry {
        slot: 0,
        info: info_a,
    }];
    let b = [ConstFnStaticEntry {
        slot: 0,
        info: info_b,
    }];
    let rep = with_slot_rep(0, 0, REP_SPECIAL);
    let packed = b"lt5cf_method\0";
    let requested = SHAPE_ID_BASE + 0x7860;
    let seeded = js_shape_seed_plain_constfn(
        requested,
        packed.as_ptr(),
        packed.len() as u32,
        1,
        1,
        rep,
        a.as_ptr(),
        1,
    );
    assert_eq!(seeded, requested);
    let keys = unsafe { canonical_keys_for_names(&[b"lt5cf_method"]) };
    assert_eq!(
        js_object_final_shape_id_for_class_keys_static_constfn(
            keys.arr() as usize as u64,
            1,
            1,
            0,
            requested,
            rep,
            a.as_ptr(),
            1,
        ),
        seeded,
        "literal seed and module init must find one shape"
    );
    let d = shapes::shape_descriptor_by_id(seeded).expect("seeded ConstFn shape");
    assert_eq!(d.constfn_infos()[0].info, info_a as usize as u64);
    assert_eq!(d.special_constfn_mask, 1);
    assert!(is_carrier(seeded));
    let other = js_object_final_shape_id_for_class_keys_static_constfn(
        keys.arr() as usize as u64,
        1,
        1,
        0,
        SHAPE_ID_BASE + 0x7861,
        rep,
        b.as_ptr(),
        1,
    );
    assert_ne!(other, seeded, "another body is another shape identity");
}

#[test]
fn constfn_seed_entry_parser_rejects_transient_duplicate_and_unsorted_bodies() {
    let permanent = crate::fn_info!(seeded_constfn_body_a, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE));
    let transient = crate::fn_info!(seeded_constfn_body_b, 0);
    let good = [ConstFnStaticEntry {
        slot: 0,
        info: permanent,
    }];
    assert!(parse_constfn_static_entries(good.as_ptr(), 1).is_some());
    let bad = [ConstFnStaticEntry {
        slot: 0,
        info: transient,
    }];
    assert!(parse_constfn_static_entries(bad.as_ptr(), 1).is_none());
    let duplicate = [good[0], good[0]];
    assert!(parse_constfn_static_entries(duplicate.as_ptr(), 2).is_none());
    let unsorted = [
        ConstFnStaticEntry {
            slot: 1,
            info: permanent,
        },
        good[0],
    ];
    let sorted = [good[0], unsorted[0]];
    assert!(parse_constfn_static_entries(sorted.as_ptr(), 2).is_some());
    assert!(parse_constfn_static_entries(unsorted.as_ptr(), 2).is_none());
    assert!(parse_constfn_static_entries(std::ptr::null(), 0).is_none());
}

fn is_carrier(id: u32) -> bool {
    shapes::test_shape_record_is_carrier(id)
}

#[test]
fn a_seed_mints_the_requested_id_and_every_later_mint_of_those_facts_resolves_to_it() {
    let requested = SHAPE_ID_BASE + 0x1234;
    let id = seed(requested, &["lt4s_a", "lt4s_b"]);
    assert_eq!(id, requested, "the seed must mint the requested static id");
    assert!(
        is_carrier(id),
        "a seeded record must carry RECORD_FLAG_EXTERNAL_CARRIER (never pruned)"
    );
    // The ordinary literal birth of the same key list (the canonical node,
    // count = live bound, proto 0) reaches the seeded record.
    let keys = unsafe { canonical_keys_for_names(&[b"lt4s_a", b"lt4s_b"]) };
    assert_eq!(shapes::shape_id_for_keys_ensure(keys.arr(), 2), requested);
    // A second seed of the same facts under another id finds the first.
    assert_eq!(
        seed(SHAPE_ID_BASE + 0x1235, &["lt4s_a", "lt4s_b"]),
        requested
    );
    assert!(
        shapes::shape_descriptor_by_id(SHAPE_ID_BASE + 0x1235).is_none(),
        "a by-facts hit must not mint the second requested id"
    );
}

/// Charter step 5 x step 4: a literal born with `F64` lanes is seeded with
/// its birth rep, so the seeded id is the shape every lazy mint of the same
/// (keys, live, rep) reaches: the literal's own class-keys mint (module init
/// of its anonymous class) and a chain of Number key-adds. A Number stored
/// into an `F64` lane of the seeded shape keeps the shape (the store check's
/// fast outcome: no generalization, the canonical double in the slot).
/// Sabotage: a seed that drops its rep mints the all-`Any` facts under the
/// static id, and the seeded record no longer carries the birth rep (nor
/// would the lazy mint below reach it).
#[test]
fn a_rep_literal_seed_is_the_shape_its_lazy_mints_reach_and_keeps_its_f64_lanes() {
    use crate::object::field_rep::{slot_rep, with_slot_rep, REP_F64};
    use crate::object::shapes::object_shape_stamp;
    let _lock = crate::gc::global_side_table_test_lock();
    const REP_SEED_ANON_CLASS_ID: u32 = 0x0075_5eed;
    let rep = with_slot_rep(with_slot_rep(0, 0, REP_F64), 1, REP_F64);
    let requested = SHAPE_ID_BASE + 0x5678;
    let id = seed_with_rep(requested, &["lt5s_a", "lt5s_b"], rep);
    assert_eq!(id, requested, "the seed must mint the requested static id");
    let record = shapes::shape_descriptor_by_id(id).expect("seeded record");
    assert_eq!(record.rep, rep, "the seeded shape must carry the birth rep");
    assert_eq!(record.proto_id, shapes::PROTO_ID_DEFAULT);

    // The literal's module-init mint WITHOUT a static id (a module whose
    // guards embed none): the lazy mint of the same facts is the seeded id.
    unsafe { crate::object::js_register_anon_shape_class_id(REP_SEED_ANON_CLASS_ID) };
    let packed = b"lt5s_a\0lt5s_b\0";
    let keys = crate::object::js_build_class_keys_array(
        REP_SEED_ANON_CLASS_ID,
        2,
        packed.as_ptr(),
        packed.len() as u32,
        0,
    ) as u64;
    assert_eq!(
        shapes::js_object_shape_id_for_class_keys(keys, 2, REP_SEED_ANON_CLASS_ID, rep),
        requested,
        "the literal's lazy mint must resolve to the seeded id"
    );
    // ...and its static request (module init with the id) hits.
    assert_eq!(
        js_object_shape_id_for_class_keys_static(
            keys,
            2,
            2,
            REP_SEED_ANON_CLASS_ID,
            requested,
            rep
        ),
        requested
    );
    // The all-`Any` sibling is other facts: never the static id.
    let any = shapes::js_object_shape_id_for_class_keys(keys, 2, REP_SEED_ANON_CLASS_ID, 0);
    assert_ne!(
        any, requested,
        "the rep is identity: Any lanes are another shape"
    );

    unsafe {
        // Number key-adds on a plain `{}` earn F64 lanes and reach the seed.
        let obj = crate::object::js_object_alloc_with_parent(0, 0, 2);
        crate::object::shapes::store_kind::premark_plain_ordinary(obj);
        for (name, v) in [("lt5s_a", 1.5f64), ("lt5s_b", 2.5)] {
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            crate::object::js_object_set_field_by_name(obj, key, v);
        }
        assert_eq!(
            object_shape_stamp(obj),
            requested,
            "a Number key-add chain of these keys must reach the seeded id"
        );
        // An F64-lane store of a Number stays on the shape.
        crate::object::store_object_field_slot(obj, 1, crate::value::INT32_TAG | 7);
        assert_eq!(
            object_shape_stamp(obj),
            requested,
            "a Number store never transitions"
        );
        let fields =
            (obj as *mut u8).add(std::mem::size_of::<crate::object::ObjectHeader>()) as *const u64;
        assert_eq!(
            *fields.add(1),
            7.0f64.to_bits(),
            "the lane holds the canonical double"
        );
        let record = shapes::shape_descriptor_by_id(requested).expect("seeded record");
        assert_eq!(
            slot_rep(record.rep, 1),
            REP_F64,
            "the lane is still F64 (not deprecated)"
        );
    }
}

/// Run `name` in a child test process with `SABOTAGE_ENV` set and require it
/// to abort with the mint's refusal message.
fn child_aborts_with_refusal(name: &str) {
    let out = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .arg(name)
        .arg("--exact")
        .arg("--nocapture")
        .arg("--test-threads=1")
        .env(SABOTAGE_ENV, "1")
        .output()
        .expect("launch the sabotaged child");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "the refused static id did not abort");
    assert!(
        stderr.contains("was refused by the shape mint"),
        "the child died without the refusal message: {stderr}"
    );
}

const SABOTAGE_ENV: &str = "PERRY_TEST_PLAIN_STATIC_REFUSAL_CHILD";

/// Ids are assigned by content, so a static id already naming OTHER facts in
/// this agent is an invariant violation: the mint aborts rather than hand a
/// counter id to code whose immediates name the static one.
#[test]
fn a_static_id_already_naming_other_facts_aborts() {
    if std::env::var_os(SABOTAGE_ENV).is_none() {
        child_aborts_with_refusal(
            "object::static_shapes::tests::a_static_id_already_naming_other_facts_aborts",
        );
        return;
    }
    let requested = SHAPE_ID_BASE + 0x2345;
    assert_eq!(seed(requested, &["lt4d_x"]), requested);
    seed(requested, &["lt4d_y"]);
}

/// A requested id outside the static band is never generated by the driver;
/// the mint aborts on it (each bad id in its own child).
#[test]
fn a_requested_id_outside_the_static_band_aborts() {
    const BAD: [u32; 3] = [0x7000_0000, STATIC_SHAPE_ID_END, STATIC_SHAPE_ID_END + 5];
    match std::env::var(SABOTAGE_ENV) {
        Err(_) => {
            for i in 0..BAD.len() {
                let out = std::process::Command::new(std::env::current_exe().expect("test binary"))
                    .arg("object::static_shapes::tests::a_requested_id_outside_the_static_band_aborts")
                    .arg("--exact")
                    .arg("--nocapture")
                    .arg("--test-threads=1")
                    .env(SABOTAGE_ENV, i.to_string())
                    .output()
                    .expect("launch the sabotaged child");
                let stderr = String::from_utf8_lossy(&out.stderr);
                assert!(
                    !out.status.success(),
                    "out-of-band {:#x} did not abort",
                    BAD[i]
                );
                assert!(
                    stderr.contains("was refused by the shape mint"),
                    "the child died without the refusal message: {stderr}"
                );
            }
        }
        Ok(i) => {
            let bad = BAD[i.parse::<usize>().unwrap_or(0)];
            seed(bad, &["lt4b_q", &format!("lt4b_{bad:x}")]);
        }
    }
}

#[test]
fn the_counter_never_mints_into_the_static_band() {
    let keys = unsafe { canonical_keys_for_names(&[b"lt4c_only"]) };
    let id = shapes::shape_id_for_keys_ensure(keys.arr(), 1);
    assert!(is_shape_id(id) && id >= STATIC_SHAPE_ID_END, "{id:#x}");
}

#[test]
fn a_class_seed_takes_the_class_prototype_identity() {
    const SEEDED_CLASS_ID: u32 = 0x0074_1c11;
    let packed = b"lt4k_a\0lt4k_b\0";
    let keys =
        crate::object::js_build_class_keys_array(SEEDED_CLASS_ID, 2, packed.as_ptr(), 14, 0) as u64;
    let requested = SHAPE_ID_BASE + 0x3456;
    let id = js_object_shape_id_for_class_keys_static(keys, 2, 2, SEEDED_CLASS_ID, requested, 0);
    let record = shapes::shape_descriptor_by_id(id).expect("seeded record");
    assert_eq!(record.proto_id, shapes::class_proto_id(SEEDED_CLASS_ID));
    // Registration is idempotent: a second registration (another module
    // importing the class) resolves to the same id.
    assert_eq!(
        js_object_shape_id_for_class_keys_static(keys, 2, 2, SEEDED_CLASS_ID, requested, 0),
        id
    );
    assert!(is_carrier(id));
}

/// The key literal's atom, as a module pool mints it at init
/// (`js_string_pool_atom`): the pointer every read site of that text passes.
fn pool_atom(text: &str) -> *const crate::StringHeader {
    let hash = super::super::key_bytes_hash(text.as_ptr(), text.len());
    crate::string::js_string_pool_atom(text.as_ptr(), text.len() as u32, hash, 0)
}

/// The megamorphic confirm as the miss entry asks it: this agent's directory,
/// the site's key as NaN-boxed bits.
unsafe fn confirmed(id: u32, key: *const crate::StringHeader, guess: usize) -> bool {
    shapes::slot_guess_confirmed(
        shapes::ordinary_dir_addr(),
        id,
        crate::JSValue::string_ptr(key as *mut _).bits(),
        guess,
    )
}

/// A seeded literal shape answers the megamorphic read's slot-guess confirm
/// (`shapes::slot_guess_confirmed`: the position bound, then ONE pointer
/// compare of the listed key against the site's key atom) exactly as a shape
/// minted after the pools ran does. The seed runs before any module's pool
/// mints its atoms, so a list that stored its own strings instead of the
/// atoms confirms nothing, and every megamorphic read of a seeded literal
/// falls to the by-name walk (lead_mega1 225.8 -> 435.8 instructions/read).
///
/// Sabotage: drop the atom mint in `build_longlived_keys_array` -> the seeded
/// record's confirms fail (the minted twin's still pass).
#[test]
fn a_seeded_literal_shape_answers_the_megamorphic_confirm_like_a_minted_one() {
    let requested = SHAPE_ID_BASE + 0x1240;
    let seeded = seed(requested, &["lt4m_a", "lt4m_k0"]);
    assert_eq!(seeded, requested, "the seed must mint the requested id");
    // Module init: the pools mint the atoms of their key literals.
    let (a, k0, k1) = (
        pool_atom("lt4m_a"),
        pool_atom("lt4m_k0"),
        pool_atom("lt4m_k1"),
    );
    // A literal minted by facts after the pools ran (the dynamic path).
    let minted_keys = unsafe { canonical_keys_for_names(&[b"lt4m_a", b"lt4m_k1"]) };
    let minted = shapes::shape_id_for_keys_ensure(minted_keys.arr(), 2);
    for (id, key, guess, what) in [
        (seeded, a, 0, "seeded `a` at 0"),
        (seeded, k0, 1, "seeded `k0` at 1"),
        (minted, a, 0, "minted `a` at 0"),
        (minted, k1, 1, "minted `k1` at 1"),
    ] {
        // POSBOUND is a fact of the record: a seeded shape (built by the
        // slab insert, like every other) carries it, nonzero, equal to its
        // definition.
        let (stored, by_facts) = shapes::test_positional_of_id(id).expect("a record");
        assert!(
            stored > 0 && stored == by_facts,
            "{what}: the record must answer by position (POSBOUND {stored}, by facts {by_facts})"
        );
        assert!(
            unsafe { confirmed(id, key, guess) },
            "{what}: the megamorphic confirm must accept the key atom"
        );
    }
    // And refutes a wrong guess or another key.
    assert!(!unsafe { confirmed(seeded, a, 1) });
    assert!(!unsafe { confirmed(seeded, k1, 1) });
}

/// A class key list answers the megamorphic confirm when the class registers
/// BEFORE any pool holding its key texts has run. Module init order is the
/// import order: a class of module A registers (`js_build_class_keys_array`,
/// then its ShapeId) while the only literal of one of its keys lives in the
/// pool of module B, which runs later. The list built at registration must
/// hold the atom B's pool then finds, or every megamorphic read of the class
/// through B's key falls to the by-name walk.
///
/// Sabotage: drop the atom mint in `build_longlived_keys_array` -> the
/// class's confirms fail.
#[test]
fn a_class_registered_before_the_pools_answers_the_megamorphic_confirm() {
    const LATE_POOL_CLASS_ID: u32 = 0x0074_1c77;
    let packed = b"ltca_x\0ltca_only_in_b\0";
    let keys = crate::object::js_build_class_keys_array(
        LATE_POOL_CLASS_ID,
        2,
        packed.as_ptr(),
        packed.len() as u32,
        0,
    ) as u64;
    let id = shapes::js_object_shape_id_for_class_keys(keys, 2, LATE_POOL_CLASS_ID, 0);
    // Module B's pool runs afterwards and mints its key literals.
    let (x, b) = (pool_atom("ltca_x"), pool_atom("ltca_only_in_b"));
    assert!(
        unsafe { confirmed(id, x, 0) },
        "class `x` at 0: the megamorphic confirm must accept the key atom"
    );
    assert!(
        unsafe { confirmed(id, b, 1) },
        "class `only_in_b` at 1: the megamorphic confirm must accept the key atom"
    );
    assert!(!unsafe { confirmed(id, x, 1) });
}

/// The finalizer sees a partly built object on Any, refuses an unwritten
/// method, and only promotes after every store. Fresh captured closures share
/// the seeded shape while preserving the receiver's current closure slot.
#[test]
fn constfn_finalizer_waits_for_stores_and_preserves_fresh_closures() {
    use crate::object::shapes::object_shape_stamp;
    let _lock = crate::gc::global_side_table_test_lock();
    let info = crate::fn_info!(seeded_constfn_body_a, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE));
    let entries = [ConstFnStaticEntry { slot: 0, info }];
    let packed = b"ltcf_final_m\0ltcf_final_x\0";
    let requested = SHAPE_ID_BASE + 0x7870;
    let final_id = js_shape_seed_plain_constfn(
        requested,
        packed.as_ptr(),
        packed.len() as u32,
        2,
        2,
        3,
        entries.as_ptr(),
        1,
    );
    assert_eq!(final_id, requested);
    let scope = crate::gc::RuntimeHandleScope::new();
    let finalize = |obj: u64| {
        js_object_finalize_constfn_static(
            obj,
            requested,
            packed.as_ptr(),
            packed.len() as u32,
            2,
            2,
            0,
            3,
            entries.as_ptr(),
            1,
        )
    };
    let mut objects = Vec::new();
    let mut closures = Vec::new();
    for capture in [11.0f64, 22.0] {
        let raw = alloc_constfn_plain_fixture(&[b"ltcf_final_m", b"ltcf_final_x"]);
        let object = scope.root_raw_mut_ptr(raw as usize as *mut crate::object::ObjectHeader);
        let plain = unsafe {
            object.with_mut_ptr::<crate::object::ObjectHeader, _>(|object_ptr| {
                object_shape_stamp(object_ptr)
            })
        };
        assert_ne!(plain, requested, "allocation must not carry SPECIAL");
        assert_eq!(shapes::shape_descriptor_by_id(plain).unwrap().rep, 0);
        assert_eq!(finalize(raw as usize as u64), raw as usize as u64);
        assert_eq!(
            unsafe {
                object.with_mut_ptr::<crate::object::ObjectHeader, _>(|object_ptr| {
                    object_shape_stamp(object_ptr)
                })
            },
            plain,
            "unwritten method must refuse"
        );
        let closure = crate::closure::js_closure_alloc(info, 1);
        let closure = scope.root_raw_mut_ptr(closure);
        unsafe {
            closure.with_mut_ptr::<crate::closure::ClosureHeader, _>(|closure_ptr| {
                crate::closure::js_closure_set_capture_bits(closure_ptr, 0, capture.to_bits())
            });
            object.with_mut_ptr::<crate::object::ObjectHeader, _>(|obj| {
                crate::object::store_object_field_slot(
                    obj,
                    0,
                    closure
                        .with_mut_ptr::<crate::closure::ClosureHeader, _>(|closure_ptr| {
                            crate::JSValue::object_ptr(closure_ptr as *mut u8)
                        })
                        .bits(),
                );
                crate::object::store_object_field_slot(obj, 1, 7.0f64.to_bits());
            });
        }
        let obj = object.with_mut_ptr::<crate::object::ObjectHeader, _>(|object_ptr| {
            finalize(object_ptr as usize as u64)
        });
        assert_eq!(
            unsafe { object_shape_stamp(obj as usize as *mut _) },
            requested
        );
        objects.push(object);
        closures.push(closure);
    }
    closures[0].with_mut_ptr::<crate::closure::ClosureHeader, _>(|closures_0_ptr| {
        closures[1].with_mut_ptr::<crate::closure::ClosureHeader, _>(|closures_1_ptr| {
            assert_ne!(closures_0_ptr, closures_1_ptr)
        })
    });
    for ((object, closure), capture) in objects.iter().zip(&closures).zip([11.0f64, 22.0]) {
        let slot = object.with_mut_ptr(|obj| crate::object::js_object_get_field(obj, 0));
        closure.with_mut_ptr::<crate::closure::ClosureHeader, _>(|closure_ptr| {
            assert_eq!(
                slot.bits() & crate::value::POINTER_MASK,
                closure_ptr as usize as u64
            )
        });
        assert_eq!(
            crate::closure::js_closure_get_capture_bits(
                (slot.bits() & crate::value::POINTER_MASK) as usize as *const _,
                0
            ),
            capture.to_bits(),
            "current receiver closure must preserve its own capture"
        );
    }
}

#[test]
fn constfn_finalizer_refuses_wrong_body_layout_and_rebindable_this() {
    let _lock = crate::gc::global_side_table_test_lock();
    let info = crate::fn_info!(seeded_constfn_body_a, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE));
    let other = crate::fn_info!(seeded_constfn_body_b, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE));
    let entries = [ConstFnStaticEntry { slot: 0, info }];
    let packed = b"ltcf_refuse_m\0";
    let scope = crate::gc::RuntimeHandleScope::new();
    // Establish that these exact birth facts promote with the supported body.
    // Otherwise every refusal below could be an unrelated kind/layout miss.
    let control = scope.root_raw_mut_ptr(alloc_constfn_plain_fixture(&[b"ltcf_refuse_m"]));
    let birth =
        unsafe { control.with_mut_ptr(|control_ptr| shapes::object_shape_stamp(control_ptr)) };
    let closure = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(info, 0));
    unsafe {
        control.with_mut_ptr(|control_ptr| {
            crate::object::store_object_field_slot(
                control_ptr,
                0,
                closure
                    .with_mut_ptr::<u8, _>(|closure_ptr| crate::JSValue::object_ptr(closure_ptr))
                    .bits(),
            )
        });
    }
    let promoted = control.with_mut_ptr::<crate::object::ObjectHeader, _>(|control_ptr| {
        js_object_finalize_constfn_static(
            control_ptr as usize as u64,
            SHAPE_ID_BASE + 0x7871,
            packed.as_ptr(),
            packed.len() as u32,
            1,
            1,
            0,
            3,
            entries.as_ptr(),
            1,
        )
    });
    assert_eq!(
        unsafe { shapes::object_shape_stamp(promoted as usize as *mut _) },
        SHAPE_ID_BASE + 0x7871,
        "the control must actually finalize before testing refusals"
    );
    assert_ne!(birth, SHAPE_ID_BASE + 0x7871);
    for (case, body, caps, count, live, class_id, rep) in [
        ("wrong body", other, 0, 1, 1, 0, 3),
        (
            "rebindable this",
            info,
            crate::closure::CAPTURES_THIS_FLAG | 1,
            1,
            1,
            0,
            3,
        ),
        ("wrong key count", info, 0, 2, 2, 0, 3),
        ("wrong live bound", info, 0, 1, 2, 0, 3),
        ("wrong prototype", info, 0, 1, 1, 0x7844, 3),
        ("wrong representation", info, 0, 1, 1, 0, 7),
    ] {
        let raw = alloc_constfn_plain_fixture(&[b"ltcf_refuse_m"]);
        let obj = scope.root_raw_mut_ptr(raw as usize as *mut crate::object::ObjectHeader);
        let closure = crate::closure::js_closure_alloc(body, caps);
        unsafe {
            obj.with_mut_ptr::<crate::object::ObjectHeader, _>(|obj_ptr| {
                crate::object::store_object_field_slot(
                    obj_ptr,
                    0,
                    crate::JSValue::object_ptr(closure as *mut u8).bits(),
                )
            });
        }
        let before = unsafe {
            obj.with_mut_ptr::<crate::object::ObjectHeader, _>(|obj_ptr| {
                shapes::object_shape_stamp(obj_ptr)
            })
        };
        assert_eq!(
            before, birth,
            "{case}: refusal must begin with the admitted control birth"
        );
        let raw = obj.with_mut_ptr::<crate::object::ObjectHeader, _>(|obj_ptr| {
            js_object_finalize_constfn_static(
                obj_ptr as usize as u64,
                SHAPE_ID_BASE + 0x7871,
                packed.as_ptr(),
                packed.len() as u32,
                count,
                live,
                class_id,
                rep,
                entries.as_ptr(),
                1,
            )
        });
        assert_eq!(
            unsafe { shapes::object_shape_stamp(raw as usize as *mut _) },
            before,
            "{case}: refusal must leave receiver unchanged"
        );
    }
}

extern "C" fn seeded_constfn_capture_body(
    closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    f64::from_bits(crate::closure::js_closure_get_capture_bits(closure, 0))
}

#[test]
fn declared_class_final_mint_keeps_birth_ordinary_and_uses_each_current_closure() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    let cid = 0x16cfa011;
    unsafe {
        crate::object::js_register_class_id(cid);
    }
    let packed = b"ltcf_class_m\0ltcf_class_x\0";
    let info = crate::fn_info!(seeded_constfn_capture_body, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE));
    let entries = [ConstFnStaticEntry { slot: 0, info }];
    let keys =
        crate::object::js_build_class_keys_array(cid, 2, packed.as_ptr(), packed.len() as u32, 0);
    let keys = scope.root_raw_mut_ptr(keys as usize as *mut crate::array::ArrayHeader);
    let ordinary = keys.with_mut_ptr::<crate::array::ArrayHeader, _>(|keys_ptr| {
        js_object_shape_id_for_class_keys_static(
            keys_ptr as usize as u64,
            2,
            2,
            cid,
            SHAPE_ID_BASE + 0x7880,
            0,
        )
    });
    let final_id = keys.with_mut_ptr::<crate::array::ArrayHeader, _>(|keys_ptr| {
        js_object_final_shape_id_for_class_keys_static_constfn(
            keys_ptr as usize as u64,
            2,
            2,
            cid,
            SHAPE_ID_BASE + 0x7881,
            3,
            entries.as_ptr(),
            1,
        )
    });
    assert_ne!(ordinary, final_id);
    assert_eq!(shapes::shape_descriptor_by_id(ordinary).unwrap().rep, 0);
    assert_eq!(shapes::shape_descriptor_by_id(final_id).unwrap().rep, 3);
    let mut closure_roots = Vec::new();
    for capture in [31.0f64, 47.0] {
        let obj = keys.with_mut_ptr::<crate::array::ArrayHeader, _>(|keys_ptr| {
            crate::object::js_object_alloc_class_inline_keys_stamped(
                cid, 0, 2, keys_ptr, ordinary, 0,
            )
        });
        let object = scope.root_raw_mut_ptr(obj);
        assert_eq!(
            unsafe { object.with_mut_ptr(|object_ptr| shapes::object_shape_stamp(object_ptr)) },
            ordinary
        );
        let closure = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(info, 1));
        unsafe {
            closure.with_mut_ptr(|closure_ptr| {
                crate::closure::js_closure_set_capture_bits(closure_ptr, 0, capture.to_bits())
            });
            object.with_mut_ptr::<crate::object::ObjectHeader, _>(|object| {
                crate::object::store_object_field_slot(
                    object,
                    0,
                    closure
                        .with_mut_ptr::<u8, _>(|closure_ptr| {
                            crate::JSValue::object_ptr(closure_ptr)
                        })
                        .bits(),
                );
                crate::object::store_object_field_slot(object, 1, capture.to_bits());
            });
        }
        let premise = shapes::shape_descriptor_by_id(ordinary).unwrap();
        assert_eq!(premise.object_kind, shapes::ShapeObjectKind::Ordinary);
        assert_eq!(premise.proto_id, shapes::class_proto_id(cid));
        let wrong_proto = object.with_mut_ptr::<crate::object::ObjectHeader, _>(|object_ptr| {
            js_object_finalize_constfn_static(
                object_ptr as usize as u64,
                final_id,
                packed.as_ptr(),
                packed.len() as u32,
                2,
                2,
                cid + 1,
                3,
                entries.as_ptr(),
                1,
            )
        });
        assert_eq!(
            unsafe { shapes::object_shape_stamp(wrong_proto as usize as *mut _) },
            ordinary,
            "a different class prototype must refuse without stamping"
        );
        let obj = object.with_mut_ptr::<crate::object::ObjectHeader, _>(|object_ptr| {
            js_object_finalize_constfn_static(
                object_ptr as usize as u64,
                final_id,
                packed.as_ptr(),
                packed.len() as u32,
                2,
                2,
                cid,
                3,
                entries.as_ptr(),
                1,
            )
        });
        assert_eq!(
            unsafe { shapes::object_shape_stamp(obj as usize as *mut _) },
            final_id
        );
        let current = crate::object::js_object_get_field(obj as usize as *mut _, 0);
        assert_eq!(
            crate::closure::js_closure_call0(
                (current.bits() & crate::value::POINTER_MASK) as usize as *const _,
                crate::closure::JsThis::UNDEFINED,
            ),
            capture
        );
        closure_roots.push(closure);
    }
    closure_roots[0].with_mut_ptr::<crate::closure::ClosureHeader, _>(|closure_roots_0_ptr| {
        closure_roots[1].with_mut_ptr::<crate::closure::ClosureHeader, _>(|closure_roots_1_ptr| {
            assert_ne!(closure_roots_0_ptr, closure_roots_1_ptr)
        })
    });
}

const ALIAS_KEYS_CHILD_ENV: &str = "PERRY_TEST_CONSTFN_ALIAS_KEYS_CHILD";

/// Store a fresh closure of `info` into slot 0 of the rooted receiver and run
/// the finalizer for the one-method shape seeded under `requested`.
fn finalize_one_method(
    scope: &crate::gc::RuntimeHandleScope,
    raw: *mut crate::object::ObjectHeader,
    info: *const crate::closure::JsFunctionInfo,
    requested: u32,
    packed: &[u8],
    entries: &[ConstFnStaticEntry],
) -> (u32, u32) {
    let obj = scope.root_raw_mut_ptr(raw);
    let closure = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(info, 0));
    let before = unsafe {
        obj.with_mut_ptr::<crate::object::ObjectHeader, _>(|obj_ptr| {
            crate::object::store_object_field_slot(
                obj_ptr,
                0,
                closure
                    .with_mut_ptr::<u8, _>(|closure_ptr| crate::JSValue::object_ptr(closure_ptr))
                    .bits(),
            );
            shapes::object_shape_stamp(obj_ptr)
        })
    };
    let after = obj.with_mut_ptr::<crate::object::ObjectHeader, _>(|obj_ptr| {
        js_object_finalize_constfn_static(
            obj_ptr as usize as u64,
            requested,
            packed.as_ptr(),
            packed.len() as u32,
            1,
            1,
            0,
            3,
            entries.as_ptr(),
            entries.len() as u32,
        )
    });
    (before, unsafe {
        shapes::object_shape_stamp(after as usize as *mut _)
    })
}

/// M1 (#11680 audit): a receiver whose keys array holds the same NAMES as the
/// record already seeded under the requested id, but is a different array,
/// passes every name-level check. The mint then misses by facts and finds the
/// id taken; before the fix that was `static_shape_id_refused_abort`. The
/// finalizer must refuse instead and leave the receiver untouched. Runs in a
/// child so the pre-fix abort fails this test rather than the test binary.
#[test]
fn constfn_finalizer_refuses_equal_names_in_a_different_keys_array() {
    const NAME: &str = "object::static_shapes::tests::constfn_finalizer_refuses_equal_names_in_a_different_keys_array";
    if std::env::var_os(ALIAS_KEYS_CHILD_ENV).is_none() {
        let out = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .arg(NAME)
            .arg("--exact")
            .arg("--nocapture")
            .arg("--test-threads=1")
            .env(ALIAS_KEYS_CHILD_ENV, "1")
            .output()
            .expect("launch the child");
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            out.status.success(),
            "the finalizer must refuse, not abort: {stderr}"
        );
        assert!(
            stdout.contains("alias-keys refusal checked"),
            "the child must actually run the scenario: {stdout}"
        );
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    let info = crate::fn_info!(seeded_constfn_body_a, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE));
    let entries = [ConstFnStaticEntry { slot: 0, info }];
    let packed = b"ltcf_alias_m\0";
    let requested = SHAPE_ID_BASE + 0x7872;
    let seeded = js_shape_seed_plain_constfn(
        requested,
        packed.as_ptr(),
        packed.len() as u32,
        1,
        1,
        3,
        entries.as_ptr(),
        1,
    );
    assert_eq!(seeded, requested);
    let scope = crate::gc::RuntimeHandleScope::new();
    // Control: the canonical-keys receiver finalizes, so the refusal below
    // is the keys identity and nothing else.
    let control = alloc_constfn_plain_fixture(&[b"ltcf_alias_m"]);
    let (_, control_after) =
        finalize_one_method(&scope, control, info, requested, packed, &entries);
    assert_eq!(control_after, requested, "control must finalize");
    // Same names, a different (non-canonical) keys array.
    let keys = unsafe {
        let _immortal = crate::gc::ImmortalLayoutScope::new();
        let arr = crate::object::alloc::build_longlived_keys_array(
            std::ptr::null_mut(),
            0,
            &[b"ltcf_alias_m"],
        );
        crate::gc::layout_init_all_pointer_slots(arr as *mut u8);
        crate::object::ObjectKeys::new(arr, 1)
    };
    let alias = crate::object::alloc_plain::alloc_plain_record_with_keys(1, keys);
    let birth = shapes::shape_descriptor_by_id(unsafe { shapes::object_shape_stamp(alias) })
        .expect("alias birth descriptor");
    let record = shapes::shape_descriptor_by_id(requested).expect("seeded record");
    assert_ne!(
        birth.keys, record.keys,
        "the fixture must use a different keys array"
    );
    assert_eq!(birth.object_kind, shapes::ShapeObjectKind::Ordinary);
    assert_eq!(birth.rep, crate::object::field_rep::REP_ANY);
    let (before, after) = finalize_one_method(&scope, alias, info, requested, packed, &entries);
    assert_ne!(before, requested);
    assert_eq!(after, before, "refusal must leave the receiver untouched");
    println!("alias-keys refusal checked");
}
