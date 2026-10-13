//! `Obj(S, memo)` typed hits (decision 129): a hit trusts the hop pairs while
//! the proof epoch the entry recorded is current, compares the holder's
//! ShapeId, and reads no class id, identity word or hop ShapeId. Contract
//! mode (always on in test builds) re-runs the full shape proof in the typed
//! body; each test below breaks one proof source the way a bug would and
//! shows the typed body aborts, then shows the real invalidation path
//! declining to the generic entry instead.
use super::tests::{pinned_shape, receiver_shape, shaped};
use super::*;

struct Fixture {
    recv: Box<ObjectHeader>,
    chain: Vec<Box<ObjectHeader>>,
    holder: Box<ObjectHeader>,
    cache: Box<PicCache>,
    token: i64,
}

/// A class receiver, two intermediate hops and an absent terminal, published
/// through the real publication path.
fn fixture(generation: u64) -> Fixture {
    let recv_shape = receiver_shape(0x0C3C_E000 + generation as u32, generation);
    let recv = shaped(recv_shape);
    let chain: Vec<Box<ObjectHeader>> = (1..3)
        .map(|i| shaped(pinned_shape(generation + i)))
        .collect();
    let holder = shaped(pinned_shape(generation + 10));
    let mut hops = NO_HOPS;
    for (i, h) in chain.iter().enumerate() {
        hops[i] = ((&**h as *const ObjectHeader) as usize, h.parent_class_id);
    }
    let w = Walk {
        holder: (&*holder as *const ObjectHeader) as usize,
        holder_shape: holder.parent_class_id,
        slot: None,
        hops,
        depth: chain.len() + 1,
        getter: 0,
    };
    let mut cache: Box<PicCache> = Box::new([0i64; crate::object::PIC_CACHE_WORDS]);
    let _admit = crate::gc::never_move::test_admit::Guard::new();
    unsafe { super::tests::publish(&mut *cache, &*recv, &w) };
    let token = (PIC_ID_TOKEN_BIT | u64::from(recv_shape)) as i64;
    Fixture {
        recv,
        chain,
        holder,
        cache,
        token,
    }
}

/// The site's record, as the publication path owns it.
unsafe fn record(f: &Fixture) -> &'static mut Site {
    &mut *((f.cache[SITE_WORD] as u64 & crate::value::POINTER_MASK) as usize as *mut Site)
}

fn leaf(f: &Fixture) -> u64 {
    unsafe { leaf_bits(&f.cache, &*f.recv, f.token) }
}

/// Run `f`, expecting the typed body's contract assertion to fire.
fn assert_contract_aborts(what: &str, f: impl FnOnce() + std::panic::UnwindSafe) {
    let err = std::panic::catch_unwind(f).expect_err(what);
    let msg = err
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| err.downcast_ref::<&str>().copied())
        .unwrap_or("");
    assert!(
        msg.contains("PERRY_CHECK_CONTRACTS") && msg.contains("Obj(S, memo)"),
        "{what}: unexpected panic {msg:?}"
    );
}

#[test]
fn typed_hit_answers_on_token_and_epoch() {
    if !crate::object::method_site::run_with_fresh_worker_gate(
        "typed_hit_answers_on_token_and_epoch",
    ) {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    let f = fixture(61_000);
    assert_eq!(leaf(&f), crate::value::TAG_UNDEFINED);
    // The differential knob declines every typed body.
    crate::object::method_site::contracts::set_force_generic_for_test(true);
    assert_eq!(leaf(&f), crate::value::TAG_HOLE);
    crate::object::method_site::contracts::set_force_generic_for_test(false);
    // The real invalidation: an epoch advance declines to the generic entry.
    crate::object::proto_validity::bump_proto_validity();
    assert_eq!(leaf(&f), crate::value::TAG_HOLE);
    // The collecting re-proof finds the shapes unchanged and records the
    // current epoch: the typed body answers again.
    let s = unsafe { record(&f) };
    let e = s.class_entry_mut(0);
    let epoch = crate::object::method_site::memo_epoch();
    assert!(unsafe { identity_matches(e) && valid_chain(e) });
    refresh_epoch(e, epoch);
    assert_eq!(leaf(&f), crate::value::TAG_UNDEFINED);
    // The holder fact is the holder's own ShapeId, epoch or not.
    let holder = &*f.holder as *const ObjectHeader as *mut ObjectHeader;
    unsafe { (*holder).parent_class_id += 1 };
    assert_eq!(leaf(&f), crate::value::TAG_HOLE);
    unsafe { (*holder).parent_class_id -= 1 };
    drop(f.chain);
}

/// Sabotage 1: a forged ShapeId memo (the hop pair records a ShapeId the hop
/// never had). The epoch admits it; contract mode aborts.
#[test]
fn forged_hop_shape_memo_aborts_in_contract_mode() {
    if !crate::object::method_site::run_with_fresh_worker_gate(
        "forged_hop_shape_memo_aborts_in_contract_mode",
    ) {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    let f = fixture(62_000);
    let s = unsafe { record(&f) };
    let e = s.class_entry_mut(0);
    unsafe { hop_block(e.hops, e.depth)[0].1 += 7 };
    let f = std::panic::AssertUnwindSafe(f);
    assert_contract_aborts("forged memo", move || {
        let _ = leaf(&f);
    });
}

/// Sabotage 2: an intermediate prototype changes shape (a key add, or an
/// accessor installed over the key: sabotage 4) after the epoch was read, and
/// the epoch does not advance. Contract mode aborts; with the advance the hit
/// declines.
#[test]
fn hop_mutation_without_epoch_advance_aborts_in_contract_mode() {
    if !crate::object::method_site::run_with_fresh_worker_gate(
        "hop_mutation_without_epoch_advance_aborts_in_contract_mode",
    ) {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    let f = fixture(63_000);
    let hop = &*f.chain[1] as *const ObjectHeader as *mut ObjectHeader;
    unsafe { (*hop).parent_class_id = pinned_shape(63_050) };
    let f = std::panic::AssertUnwindSafe(f);
    assert_contract_aborts("hop mutated, epoch kept", || {
        let _ = leaf(&f);
    });
    crate::object::proto_validity::bump_proto_validity();
    assert_eq!(
        leaf(&f),
        crate::value::TAG_HOLE,
        "the advanced epoch declines"
    );
}

/// Sabotage 3: an intermediate prototype is re-linked (`setPrototypeOf` on
/// a hop restamps its ShapeId, whose identity names the link) and the epoch
/// does not advance. Contract mode aborts; with the advance the hit declines.
/// A relink of the receiver's own class link is the identity word's fact:
/// the typed body compares it and declines.
#[test]
fn hop_relink_without_epoch_advance_aborts_in_contract_mode() {
    if !crate::object::method_site::run_with_fresh_worker_gate(
        "hop_relink_without_epoch_advance_aborts_in_contract_mode",
    ) {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    let f = fixture(64_000);
    let hop = &*f.chain[0] as *const ObjectHeader as *mut ObjectHeader;
    let original = unsafe { (*hop).parent_class_id };
    unsafe { (*hop).parent_class_id = pinned_shape(64_050) };
    let f = std::panic::AssertUnwindSafe(f);
    assert_contract_aborts("hop relinked, epoch kept", || {
        let _ = leaf(&f);
    });
    crate::object::proto_validity::bump_proto_validity();
    assert_eq!(
        leaf(&f),
        crate::value::TAG_HOLE,
        "the advanced epoch declines"
    );
    unsafe { (*hop).parent_class_id = original };
    let s = unsafe { record(&f) };
    refresh_epoch(
        s.class_entry_mut(0),
        crate::object::method_site::memo_epoch(),
    );
    assert_eq!(leaf(&f), crate::value::TAG_UNDEFINED);
    let other = shaped(pinned_shape(64_070));
    let pid = crate::object::shapes::shape_proto_id(f.recv.parent_class_id).unwrap();
    crate::object::shapes::write_identity_word(
        pid,
        crate::value::POINTER_TAG | (&*other as *const ObjectHeader) as u64,
    );
    assert_eq!(leaf(&f), crate::value::TAG_HOLE, "a class relink declines");
    drop(other);
}
