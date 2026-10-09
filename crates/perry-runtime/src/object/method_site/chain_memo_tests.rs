use super::*;

#[test]
fn s7b_chain_memo_rechecks_the_class_identity_word() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let cid = 190_707;
    let recv = crate::object::js_object_alloc(cid, 0);
    let holder = crate::object::js_object_alloc(0, 1);
    crate::object::js_object_set_field(holder, 0, crate::JSValue::number(42.0));
    let pid = crate::object::shapes::class_identity_proto_id(cid);
    crate::object::shapes::write_identity_word(
        pid,
        crate::value::js_nanbox_pointer(holder as i64).to_bits(),
    );
    let mut memo: ChainMemo = unsafe { std::mem::zeroed() };
    let way = &mut memo.ways[0];
    way.recv = unsafe { header_word(recv as usize) };
    way.depth = 1;
    way.hops[0] = holder as usize;
    way.words[0] = unsafe { header_word(holder as usize) };
    assert_eq!(
        unsafe { ways_lookup(&memo, false, recv as usize, b"m") }.map(|v| v.0),
        Some(42.0f64.to_bits()),
        "the recorded way must hit before replacement"
    );
    let other = crate::object::js_object_alloc(0, 1);
    crate::object::shapes::write_identity_word(
        pid,
        crate::value::js_nanbox_pointer(other as i64).to_bits(),
    );
    assert_eq!(unsafe { header_word(recv as usize) }, memo.ways[0].recv);
    assert_eq!(
        unsafe { header_word(holder as usize) },
        memo.ways[0].words[0]
    );
    assert!(
        unsafe { ways_lookup(&memo, false, recv as usize, b"m") }.is_none(),
        "an unchanged receiver and holder cannot validate a displaced identity edge"
    );
}
