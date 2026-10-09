use super::*;
use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use crate::value::{js_nanbox_pointer, js_nanbox_string};

fn word() -> *mut u64 {
    Box::leak(Box::new([0u64; 2])).as_mut_ptr()
}
fn literal<'s>(scope: &'s RuntimeHandleScope, site: *mut u64, pattern: &str) -> RuntimeHandle<'s> {
    let source = scope.root_string_ptr(super::super::js_string_from_str(pattern));
    let flags = scope.root_string_ptr(super::super::js_string_from_str("g"));
    scope.root_raw_mut_ptr(source.with_const_ptr(|source| {
        flags.with_const_ptr(|flags| js_regexp_literal(source, flags, site as i64))
    }))
}
fn data(re: &RuntimeHandle<'_>) -> usize {
    re.with_const_ptr::<RegExpHeader, _>(|re| super::super::regexp_data_ptr(re) as usize)
}

#[test]
fn literal_fresh() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = RuntimeHandleScope::new();
    let site = word();
    let a = literal(&scope, site, "fresh");
    let b = literal(&scope, site, "fresh");
    assert_ne!(
        a.with_const_ptr::<RegExpHeader, _>(|p| p),
        b.with_const_ptr::<RegExpHeader, _>(|p| p)
    );
    assert_eq!(data(&a), data(&b));
    assert_eq!(
        unsafe { *site & crate::value::POINTER_MASK },
        data(&a) as u64,
        "the site holds data, never the object"
    );
    let _ = literal(&scope, site, "fresh");
    assert_eq!(data(&a) as u64, unsafe {
        *site & crate::value::POINTER_MASK
    });
}

#[test]
fn literal_state() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = RuntimeHandleScope::new();
    let site = word();
    let a = literal(&scope, site, "a");
    let b = literal(&scope, site, "a");
    let input = scope.root_string_ptr(super::super::js_string_from_str("aa"));
    assert_ne!(
        a.with_const_ptr(|p| input.with_const_ptr(|s| super::super::js_regexp_test(p, s))),
        0
    );
    assert_eq!(
        a.with_const_ptr(|p| super::super::regex_last_index_offset(p)),
        1
    );
    assert_eq!(
        b.with_const_ptr(|p| super::super::regex_last_index_offset(p)),
        0
    );
    let c = literal(&scope, site, "a");
    assert_eq!(
        c.with_const_ptr(|p| super::super::regex_last_index_offset(p)),
        0
    );
    assert_eq!(data(&a), data(&c));
}

#[test]
fn literal_compile_replaces_only_one_instance_matcher() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = RuntimeHandleScope::new();
    let site = word();
    let a = literal(&scope, site, "before");
    let b = literal(&scope, site, "before");
    let original = data(&a);
    let source = scope.root_string_ptr(super::super::js_string_from_str("after"));
    let flags = scope.root_string_ptr(super::super::js_string_from_str("i"));
    a.with_mut_ptr(|a| {
        source.with_const_ptr::<StringHeader, _>(|s| {
            flags.with_const_ptr::<StringHeader, _>(|f| {
                super::super::js_regexp_compile_value(
                    a,
                    js_nanbox_string(s as i64),
                    js_nanbox_string(f as i64),
                )
            })
        })
    });
    assert_ne!(data(&a), original);
    assert_eq!(data(&b), original);
    assert_eq!(
        unsafe { *site & crate::value::POINTER_MASK },
        original as u64
    );
    let c = literal(&scope, site, "before");
    assert_eq!(data(&c), original);
    for re in [&b, &c] {
        let source = re.with_const_ptr(|p| super::super::js_regexp_get_source(p));
        assert_eq!(super::super::string_as_str(source), "before");
    }
    assert!(super::super::regexp_data_of(
        a.with_const_ptr::<RegExpHeader, _>(|p| js_nanbox_pointer(p as i64))
    )
    .is_some());
}

#[test]
fn literal_worker_never_reads_or_publishes_the_primary_site_word() {
    let _lock = crate::gc::global_side_table_test_lock();
    // A deliberately invalid foreign heap address proves the worker guard
    // runs before even reading/dereferencing the site's cached data.
    let site = Box::leak(Box::new([u64::MAX; 2])).as_mut_ptr() as usize;
    std::thread::spawn(move || {
        let agent = crate::agent::enter_worker_agent();
        let scope = RuntimeHandleScope::new();
        let a = literal(&scope, site as *mut u64, "worker");
        let b = literal(&scope, site as *mut u64, "worker");
        assert_eq!(
            data(&a),
            data(&b),
            "worker fallback uses its thread compile cache"
        );
        assert_ne!(
            a.with_const_ptr::<RegExpHeader, _>(|p| p),
            b.with_const_ptr::<RegExpHeader, _>(|p| p)
        );
        assert_eq!(unsafe { *(site as *mut u64) }, u64::MAX);
        assert_eq!(unsafe { *(site as *mut u64).add(1) }, u64::MAX);
        crate::agent::retire_agent(agent);
    })
    .join()
    .unwrap();
}

#[test]
fn two_literal_sites_with_equal_length_patterns_keep_their_own_data() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = RuntimeHandleScope::new();
    let site_a = word();
    let site_b = word();
    for _ in 0..2 {
        let a = literal(&scope, site_a, "a.c");
        let b = literal(&scope, site_b, "x.z");
        assert_ne!(data(&a), data(&b));
        for (re, expected) in [(&a, "a.c"), (&b, "x.z")] {
            let source = re.with_const_ptr(|p| super::super::js_regexp_get_source(p));
            assert_eq!(super::super::string_as_str(source), expected);
        }
    }
}

/// Word 1 is the header word generated code stamps on its inline births: the
/// first evaluation publishes exactly the runtime birth's own header word,
/// after word 0, and later evaluations leave it alone.
#[test]
fn literal_site_publishes_the_inline_birth_header_word() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = RuntimeHandleScope::new();
    let site = word();
    assert_eq!(unsafe { *site.add(1) }, 0);
    let a = literal(&scope, site, "header");
    let header = a.with_const_ptr::<RegExpHeader, _>(|re| unsafe { (re as *const u64).read() });
    assert_ne!(unsafe { *site }, 0, "word 0 holds the data");
    assert_eq!(
        unsafe { *site.add(1) },
        header,
        "word 1 is the birth's header word"
    );
    let shape = (header >> 32) as u32;
    let descriptor = crate::object::shapes::shape_descriptor_by_id(shape).expect("a live shape");
    assert_eq!(descriptor.live_inline_slot_count, 2);
    let b = literal(&scope, site, "header");
    assert_eq!(
        b.with_const_ptr::<RegExpHeader, _>(|re| unsafe { (re as *const u64).read() }),
        header,
        "every birth of the site carries the published header word"
    );
}

/// A primed site's evaluation is the inline birth (`inline_birth`): the next
/// 40 bytes of the inline arena, the published header word, the data from
/// word 0 and lastIndex +0, born white (no seed) when no mark is live.
#[test]
fn primed_literal_is_born_in_the_inline_arena() {
    if !crate::object::method_site::run_with_fresh_worker_gate(
        "primed_literal_is_born_in_the_inline_arena",
    ) {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = RuntimeHandleScope::new();
    let site = word();
    let _first = literal(&scope, site, "inline");
    let header = unsafe { *site.add(1) };
    assert_ne!(header, 0, "the first evaluation publishes word 1");
    // The operands first: their allocations move the arena.
    let source = scope.root_string_ptr(super::super::js_string_from_str("inline"));
    let flags = scope.root_string_ptr(super::super::js_string_from_str("g"));
    let state = crate::arena::js_inline_arena_state();
    let (data, offset) = unsafe { ((*state).data as usize, (*state).offset) };
    assert!(
        offset + super::BIRTH_BYTES <= unsafe { (*state).size },
        "SUBJECT-LIVE CHECK: the open block has room for the birth"
    );
    let re = scope.root_raw_mut_ptr(source.with_const_ptr(|source| {
        flags.with_const_ptr(|flags| js_regexp_literal(source, flags, site as i64))
    }));
    let at = re.with_const_ptr::<RegExpHeader, _>(|p| p as usize);
    assert_eq!(
        at,
        data + offset + crate::gc::GC_HEADER_SIZE,
        "the next inline cell"
    );
    assert_eq!(unsafe { (*state).offset }, offset + super::BIRTH_BYTES);
    unsafe {
        let gc = &*((at - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader);
        assert_eq!(gc.obj_type, crate::gc::GC_TYPE_OBJECT);
        assert_eq!(gc.gc_flags, crate::gc::GC_FLAG_ARENA);
        assert_eq!(gc._reserved, crate::gc::OBJ_FLAG_PLAIN_ORDINARY);
        assert_eq!(gc.size as usize, super::BIRTH_BYTES);
        assert_eq!((at as *const u64).read(), header);
        assert!((*(at as *const RegExpHeader)).meta.is_null());
        let slots = (at + std::mem::size_of::<RegExpHeader>()) as *const u64;
        assert_eq!(slots.read(), *site, "slot 0 is word 0's data");
        assert_eq!(slots.add(1).read(), 0.0f64.to_bits());
    }
    let input = scope.root_string_ptr(super::super::js_string_from_str("an inline birth"));
    assert_ne!(
        re.with_const_ptr(|p| input.with_const_ptr(|s| super::super::js_regexp_test(p, s))),
        0
    );
}

/// While a mark is live the inline birth is born black and seeded (after
/// both slots hold their values), as an emitted `new` is.
#[test]
fn primed_literal_born_during_a_mark_is_seeded() {
    if !crate::object::method_site::run_with_fresh_worker_gate(
        "primed_literal_born_during_a_mark_is_seeded",
    ) {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = RuntimeHandleScope::new();
    let site = word();
    let _first = literal(&scope, site, "black");
    let state = crate::arena::js_inline_arena_state();
    let seeds = unsafe { &mut *(*state).birth_seeds.cast::<Vec<*mut crate::gc::GcHeader>>() };
    let queued = seeds.len();
    let source = scope.root_string_ptr(super::super::js_string_from_str("black"));
    let regex_flags = scope.root_string_ptr(super::super::js_string_from_str("g"));
    let birth_flags = crate::gc::gc_birth_flags_address() as *mut u8;
    // SAFETY: this thread's live birth-flags cell; restored below. Nothing
    // else allocates while it is set.
    unsafe { birth_flags.write(crate::gc::GC_FLAG_MARKED) };
    let born = source.with_const_ptr(|source| {
        regex_flags.with_const_ptr(|flags| js_regexp_literal(source, flags, site as i64))
    });
    unsafe { birth_flags.write(0) };
    let header = (born as usize - crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
    let seeded = seeds.len() == queued + 1 && seeds.last() == Some(&header);
    if seeded {
        seeds.pop();
    }
    unsafe { (*header).gc_flags &= !crate::gc::GC_FLAG_MARKED };
    assert!(seeded, "a black inline birth is seeded");
}

/// A full block is the runtime birth: the inline birth refuses rather than
/// bump past the open block. Word 1 is set to a marked image here, so a birth
/// that stamped it would show.
#[test]
fn primed_literal_with_no_room_takes_the_runtime_birth() {
    if !crate::object::method_site::run_with_fresh_worker_gate(
        "primed_literal_with_no_room_takes_the_runtime_birth",
    ) {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = RuntimeHandleScope::new();
    let site = word();
    let _first = literal(&scope, site, "full");
    let header = unsafe { *site.add(1) };
    // A class id the birth never carries.
    unsafe { *site.add(1) = header ^ 0x8000_0000 };
    // The operands first: their allocations resync the inline limit.
    let source = scope.root_string_ptr(super::super::js_string_from_str("full"));
    let flags = scope.root_string_ptr(super::super::js_string_from_str("g"));
    let state = crate::arena::js_inline_arena_state();
    unsafe { (*state).size = (*state).offset };
    let re = scope.root_raw_mut_ptr(source.with_const_ptr(|source| {
        flags.with_const_ptr(|flags| js_regexp_literal(source, flags, site as i64))
    }));
    unsafe { *site.add(1) = header };
    assert_eq!(
        re.with_const_ptr::<RegExpHeader, _>(|p| unsafe { (p as *const u64).read() }),
        header,
        "the runtime birth carries the birth shape's own header"
    );
}
