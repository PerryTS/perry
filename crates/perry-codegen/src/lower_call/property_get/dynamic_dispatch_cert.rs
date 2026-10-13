//! Certified class-method selection (decision 129 slice 2, `Obj(S, memo)`
//! for a method call; runtime side `object/method_site/cert.rs`).
//!
//! Child module of `dynamic_dispatch.rs`. Before the call's arguments the
//! site compares the receiver against its method certificates (the record
//! `MethodCertSite`: one proof epoch, `METHOD_CERT_WAYS` receiver words): each
//! is a `PristineReceiverFact::Shape { receiver_word, proof_word, proof_epoch }`
//! whose hit (two compares, one load) names the body the receiver resolves
//! the method to. Every miss calls the ONE generic entry,
//! `js_method_site_certify`, which re-resolves and rewrites the certificate.
//! The selected body is compared against this site's compiled bodies
//! (`<method>__eclo$info`, the info of the function object a class
//! prototype holds for that method); the answer is the case to call, or 0
//! for the site's generic arm. No own-property probe, holder load or class
//! directory read remains on the path.

use super::*;
use crate::builtin_proto::{emit_pristine_receiver, PristineReceiverFact};
use crate::types::{I8, PTR};

/// Contract-check builds (`PERRY_CHECK_CONTRACTS=1` while compiling, the one
/// knob `codegen::helpers::check_contracts`): each certified hit also calls
/// the witness `js_method_site_cert_contract_check`, which re-proves the
/// certificate on the live receiver and reports through the runtime's one
/// `contract_violated`. Release code emits nothing.
fn contract_mode() -> bool {
    crate::codegen::helpers::check_contracts()
}

/// Negative control, contract-check builds only:
/// `PERRY_METHOD_CERT_SABOTAGE=epoch` makes the hit ignore the proof epoch,
/// `=word` forges the receiver word as the proof word. The witness must then
/// abort; a release build never reads the knob.
fn sabotage() -> Option<String> {
    contract_mode()
        .then(|| std::env::var("PERRY_METHOD_CERT_SABOTAGE").ok())
        .flatten()
}

/// Emit the selection; returns an `i32`: `i + 1` to call implementor `i`
/// directly (only a `cased` one), 0 for the generic arm. `recv` is the boxed receiver and is not
/// used after the generic entry's call.
pub(super) fn select(
    ctx: &mut FnCtx<'_>,
    recv: &str,
    method_id: &str,
    implementors: &[(u32, String)],
    cased: &[bool],
) -> String {
    let word = crate::lower_call::method_override::emit_inline_direct_method_word_probe(ctx, recv);
    select_with_word(ctx, recv, method_id, implementors, cased, &word)
}

/// Consume an already admitted receiver word. Static and dynamic sites share
/// this record, fact emitter and single miss entry.
pub(super) fn select_with_word(
    ctx: &mut FnCtx<'_>,
    recv: &str,
    method_id: &str,
    implementors: &[(u32, String)],
    cased: &[bool],
    word: &str,
) -> String {
    use crate::runtime_abi as abi;
    let cid = ctx.block().trunc(I64, &word, I32);
    let no = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let name = crate::expr::inline_cache_global_name(ctx, no);
    ctx.ic_globals.push(name.clone());
    let slot = format!("@{name}");
    let member = ctx.new_block("mcert.member");
    let pre = ctx.new_block("mcert.record");
    let head = ctx.new_block("mcert.way");
    let next = ctx.new_block("mcert.next");
    let hit = ctx.new_block("mcert.hit");
    let certify = ctx.new_block("mcert.certify");
    let got = ctx.new_block("mcert.got");
    let pre_l = ctx.block_label(pre);
    let (member_l, head_l, next_l, hit_l, certify_l, got_l) = (
        ctx.block_label(member),
        ctx.block_label(head),
        ctx.block_label(next),
        ctx.block_label(hit),
        ctx.block_label(certify),
        ctx.block_label(got),
    );
    // Only a receiver whose class id names a compiled body consults the
    // record: any other answer is the generic arm's anyway.
    let start_l = ctx.block().label.clone();
    let mut ids = std::collections::HashSet::new();
    let mut switch = format!("switch i32 {cid}, label %{got_l} [");
    for (i, (id, _)) in implementors.iter().enumerate() {
        if cased[i] && ids.insert(*id) {
            switch.push_str(&format!(" i32 {id}, label %{member_l}"));
        }
    }
    switch.push_str(" ]");
    ctx.block().emit_raw(switch);
    ctx.block().mark_terminated();
    ctx.current_block = member;
    let site = ctx.block().load_atomic_acquire(PTR, &slot, 8);
    let present = ctx.block().icmp_ne(PTR, &site, "null");
    // A worker's receivers resolve against its own heap's prototypes.
    let workers = ctx
        .block()
        .load_atomic_seq_cst(I8, "@PERRY_METHOD_SITE_WORKERS_PRESENT", 1);
    let primary = ctx.block().icmp_eq(I8, &workers, "0");
    let enabled = ctx.block().and(I1, &present, &primary);
    ctx.block().cond_br(&enabled, &pre_l, &certify_l);
    ctx.current_block = pre;
    // Every way was proven at the record's one epoch.
    let ep = ctx.block().gep(
        I8,
        &site,
        &[(I64, &abi::METHOD_CERT_EPOCH_OFFSET.to_string())],
    );
    let proof_epoch = ctx.block().load(I64, &ep);
    let epoch_operand = if sabotage().as_deref() == Some("epoch") {
        ctx.block()
            .load_atomic_unordered(I64, "@PERRY_PROTO_VALIDITY", 8)
    } else {
        proof_epoch.clone()
    };
    ctx.block().br(&head_l);
    // One compact loop over the ways: the same emitted fact for each.
    ctx.current_block = head;
    let way = ctx.block().next_reg();
    let way_next = ctx.block().next_reg();
    ctx.block().emit_raw(format!(
        "{way} = phi i64 [ 0, %{pre_l} ], [ {way_next}, %{next_l} ]"
    ));
    let off = ctx
        .block()
        .mul(I64, &way, &abi::METHOD_CERT_WAY_SIZE.to_string());
    let at = ctx.block().gep(I8, &site, &[(I64, &off)]);
    let wp = ctx.block().gep(
        I8,
        &at,
        &[(
            I64,
            &(abi::METHOD_CERT_WAYS_OFFSET + abi::METHOD_CERT_WORD_OFFSET).to_string(),
        )],
    );
    let proof_word = ctx.block().load(I64, &wp);
    let receiver_word = if sabotage().as_deref() == Some("word") {
        proof_word.clone()
    } else {
        word.to_string()
    };
    let holds = emit_pristine_receiver(
        ctx.block(),
        &PristineReceiverFact::Shape {
            receiver_word: &receiver_word,
            proof_word: &proof_word,
            proof_epoch: &epoch_operand,
        },
    );
    ctx.block().cond_br(&holds, &hit_l, &next_l);
    ctx.current_block = next;
    ctx.block()
        .emit_raw(format!("{way_next} = add i64 {way}, 1"));
    let more = ctx
        .block()
        .icmp_ult(I64, &way_next, &abi::METHOD_CERT_WAYS.to_string());
    ctx.block().cond_br(&more, &head_l, &certify_l);
    ctx.current_block = hit;
    let ip = ctx.block().gep(
        I8,
        &at,
        &[(
            I64,
            &(abi::METHOD_CERT_WAYS_OFFSET + abi::METHOD_CERT_INFO_OFFSET).to_string(),
        )],
    );
    let info = ctx.block().load(I64, &ip);
    if contract_mode() {
        ctx.block().call_void(
            "js_method_site_cert_contract_check",
            &[
                (DOUBLE, recv),
                (I64, method_id),
                (I64, &proof_word),
                (I64, &proof_epoch),
                (I64, &info),
            ],
        );
    }
    let hit_end = ctx.block().label.clone();
    ctx.block().br(&got_l);
    // Every miss: the one generic entry re-resolves and rewrites.
    ctx.current_block = certify;
    let certified = ctx.block().call(
        I64,
        "js_method_site_certify",
        &[(PTR, &slot), (DOUBLE, recv), (I64, method_id)],
    );
    let certify_end = ctx.block().label.clone();
    ctx.block().br(&got_l);
    ctx.current_block = got;
    let body = ctx.block().phi(
        I64,
        &[
            ("0", &start_l),
            (&info, &hit_end),
            (&certified, &certify_end),
        ],
    );
    // Which compiled body: the receiver's class names the candidate case
    // (first match, as the tower always did); the certified body must be it.
    let done = ctx.new_block("mcert.selected");
    let done_l = ctx.block_label(done);
    let start_l = ctx.block().label.clone();
    let mut seen = std::collections::HashSet::new();
    let cases: Vec<(usize, usize)> = implementors
        .iter()
        .enumerate()
        .filter(|&(i, (id, _))| cased[i] && seen.insert(*id))
        .map(|(i, _)| (i, ctx.new_block("mcert.case")))
        .collect();
    let mut switch = format!("switch i32 {cid}, label %{done_l} [");
    for &(i, idx) in &cases {
        let id = implementors[i].0;
        switch.push_str(&format!(" i32 {id}, label %{}", ctx.block_label(idx)));
    }
    switch.push_str(" ]");
    ctx.block().emit_raw(switch);
    ctx.block().mark_terminated();
    let mut answers = vec![("0".to_string(), start_l)];
    for (i, idx) in cases {
        ctx.current_block = idx;
        let expected = ctx
            .block()
            .fn_info_ref(&format!("{}__eclo", implementors[i].1));
        let expected = ctx.block().ptrtoint(&expected, I64);
        let same = ctx.block().icmp_eq(I64, &body, &expected);
        let answer = ctx
            .block()
            .select(I1, &same, I32, &(i + 1).to_string(), "0");
        let end = ctx.block().label.clone();
        ctx.block().br(&done_l);
        answers.push((answer, end));
    }
    ctx.current_block = done;
    let incoming: Vec<_> = answers
        .iter()
        .map(|(v, l)| (v.as_str(), l.as_str()))
        .collect();
    ctx.block().phi(I32, &incoming)
}
