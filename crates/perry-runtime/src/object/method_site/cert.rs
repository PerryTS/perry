//! Method certificates: `Obj(S, memo)` for a class method call (decision
//! 129, slice 2; sitememo phase 3).
//!
//! A class-dispatch site (`perry-codegen`
//! `lower_call/property_get/dynamic_dispatch_cert.rs`) owns one record of a
//! method site's footprint, [`MethodCertSite`]: certificates recorded at one
//! `PERRY_PROTO_VALIDITY` value, [`MethodCertSite::epoch`]. Way `k` claims
//! that every receiver whose header word is `ways[k].word` (`class_id |
//! ShapeId << 32`) resolves the site's method name to a function object of
//! the body `ways[k].info`. The emitted hit is exactly
//! [`PristineReceiverFact::Shape`]`{ receiver_word, proof_word: ways[k].word,
//! proof_epoch: epoch }.holds()`: two compares and one load, with no per-use
//! rebuild, holder load or own-property probe. Any miss calls the ONE generic
//! entry, [`js_method_site_certify`], which re-resolves and rewrites the
//! certificate.
//!
//! The ways share one epoch: a class-dispatch site is polymorphic over the
//! classes that implement its method (Zod's `_parse` sites see five receiver
//! words), and per-way epochs in the two six-word method entries left room
//! for two receivers. The same fourteen words hold six certificates.
//!
//! # Why the fact holds while the two words match
//!
//! * The receiver's ShapeId names its own keys, descriptors and
//!   [[Prototype]] link (#11342), so "the name is not an own property" and
//!   "the chain starts at this object" are facts of `word`.
//! * Every object from that first prototype to the holder is a MARKED
//!   prototype (`proto_validity::object_is_marked_prototype`, checked here;
//!   a declared class prototype is marked at birth). A structural change of
//!   a marked object (a key added, deleted or redefined, its own
//!   [[Prototype]] relinked) advances `PERRY_PROTO_VALIDITY`, and so does a
//!   method replaced in place on one (`builtin_proto::note_value_store`;
//!   store PICs never learn a marked prototype's shape, so every store to it
//!   reaches that funnel).
//!
//! An unmarked hop, an accessor, an own property, a Proxy or any other
//! receiver the chain cannot answer certifies body 0: its hits take the
//! site's generic arm, which is correct for every receiver.
//!
//! # GC
//!
//! A certificate holds no heap reference: `info` is the `JsFunctionInfo` of a
//! body in the permanent executable image ([`FN_PERMANENT_IMAGE`]: static
//! data, never moved or freed) and `word`/`epoch` are scalars. The record
//! lives in the IC arena; nothing is registered for the root scan.
//!
//! [`FN_PERMANENT_IMAGE`]: crate::codegen_abi::FN_PERMANENT_IMAGE

use super::*;
use crate::object::builtin_proto::{proof_epoch, receiver_word, PristineReceiverFact};
use crate::object::native_call_method::class_holder::{
    chain_method, next_holder, ChainMethod, MethodKey,
};

/// Ways per certified site.
pub const METHOD_CERT_WAYS: usize = crate::codegen_abi::METHOD_CERT_WAYS;

/// One certificate: a receiver word and the body it resolves to.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CertWay {
    /// `class_id | ShapeId << 32`; [`METHOD_SITE_EMPTY`] when unused.
    pub word: u64,
    /// The body's `JsFunctionInfo` (permanent image).
    pub info: u64,
}

/// A certified class-dispatch site's record. **Offsets are baked into
/// emitted code** (`perry_abi::METHOD_CERT_*`).
#[repr(C)]
pub struct MethodCertSite {
    /// The `PERRY_PROTO_VALIDITY` value every way was proven at.
    pub epoch: u64,
    /// Evictions so far (the replacement cursor).
    next: u64,
    pub ways: [CertWay; METHOD_CERT_WAYS],
}

/// The emitted `@perry_ic_N` slot of a certified site.
pub type MethodCertSlot = *mut MethodCertSite;

const EMPTY_WAY: CertWay = CertWay {
    word: METHOD_SITE_EMPTY,
    info: 0,
};

#[cfg(target_pointer_width = "64")]
const _: () = {
    // The certified site carries exactly a method site's record footprint.
    assert!(std::mem::size_of::<MethodCertSite>() == std::mem::size_of::<MethodSite>());
    assert!(
        std::mem::offset_of!(MethodCertSite, epoch) == crate::codegen_abi::METHOD_CERT_EPOCH_OFFSET
    );
    assert!(
        std::mem::offset_of!(MethodCertSite, ways) == crate::codegen_abi::METHOD_CERT_WAYS_OFFSET
    );
    assert!(std::mem::size_of::<CertWay>() == crate::codegen_abi::METHOD_CERT_WAY_SIZE);
    assert!(std::mem::offset_of!(CertWay, word) == crate::codegen_abi::METHOD_CERT_WORD_OFFSET);
    assert!(std::mem::offset_of!(CertWay, info) == crate::codegen_abi::METHOD_CERT_INFO_OFFSET);
};

/// Evictions after which a site stops recording new receivers (it has more
/// receiver words than ways); an epoch change still re-proves its ways.
const METHOD_CERT_MAX_EVICTIONS: u64 = 16;

/// Record `word -> info` at `epoch`. A record proven at another epoch holds
/// nothing any more and is emptied first. The word is written last.
unsafe fn record(slot: *mut MethodCertSlot, word: u64, info: u64, epoch: u64) {
    let site = crate::object::pic_slot_resolve_init(slot, |fresh| {
        // GC_STORE_AUDIT(INIT): scalar words only.
        std::ptr::write(
            fresh,
            MethodCertSite {
                epoch: 0,
                next: 0,
                ways: [EMPTY_WAY; METHOD_CERT_WAYS],
            },
        );
    });
    if site.is_null() {
        return;
    }
    let site = &mut *site;
    if site.epoch != epoch {
        for w in &mut site.ways {
            w.word = METHOD_SITE_EMPTY;
        }
        site.epoch = epoch;
    }
    let idx = match site
        .ways
        .iter()
        .position(|w| w.word == word)
        .or_else(|| site.ways.iter().position(|w| w.word == METHOD_SITE_EMPTY))
    {
        Some(i) => i,
        None if site.next >= METHOD_CERT_MAX_EVICTIONS => return,
        None => {
            let i = (site.next as usize) % METHOD_CERT_WAYS;
            site.next += 1;
            i
        }
    };
    let w = &mut site.ways[idx];
    w.word = METHOD_SITE_EMPTY;
    w.info = info;
    w.word = word;
}

/// The one generic entry of a certified method site: re-resolve `name` on
/// `recv`, rewrite the site's certificate for the receiver's word, and
/// return the certified body's `JsFunctionInfo` address (0 when no body is
/// certified: the site's generic arm). The site
/// compares the answer against its compiled bodies; anything else takes its
/// generic call.
///
/// Runs before the call's arguments (the method is read first), under a
/// collection-suppress scope: it never collects, throws or runs code.
///
/// # Safety
/// `slot` is null or a live method-site slot.
#[no_mangle]
#[inline(never)]
pub unsafe extern "C-unwind" fn js_method_site_certify(
    slot: *mut MethodCertSlot,
    recv: f64,
    method_id: i64,
) -> u64 {
    if !miss_entry::is_site_receiver(recv)
        || crate::agent::current_agent() != crate::agent::PRIMARY_AGENT
    {
        return 0;
    }
    let addr = (recv.to_bits() & crate::value::POINTER_MASK) as usize;
    let Some(obj) = ordinary_receiver(addr) else {
        return 0;
    };
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(name_ref) = crate::string::perry_string_ref_from_dispatch_id(method_id, &mut scratch)
    else {
        return 0;
    };
    let name = std::slice::from_raw_parts(name_ref.ptr, name_ref.len);
    if name_refused(name) {
        return 0;
    }
    let _no_move = crate::gc::GcSuppressScope::new();
    // The epoch is read BEFORE the walk: anything that moves it after this
    // point leaves the certificate conservatively stale.
    let epoch = proof_epoch();
    // A receiver no body is certified for (an own property, an accessor, an
    // unmarked hop) is recorded with body 0: its hit selects the generic arm
    // directly, which is correct for every receiver, instead of calling here
    // again. An own key is a fact of the word itself.
    let answer = prove(obj, name).map_or(0, |info| info as *const _ as u64);
    if WORKER_AGENTS_EXIST.load(Ordering::SeqCst) == 0 {
        record(slot, receiver_word(obj), answer, epoch);
    }
    answer
}

/// `P::prove` for a certificate: the body `name` resolves to on `obj`, when
/// the receiver's word and the marked chain determine it (module docs).
/// Also the contract-mode assertion's proof, so the two cannot drift.
///
/// # Safety
/// `obj` is a live ordinary object ([`ordinary_receiver`]).
pub(crate) unsafe fn prove(
    obj: *const ObjectHeader,
    name: &[u8],
) -> Option<&'static crate::closure::JsFunctionInfo> {
    // Absent from the receiver: a fact of its ShapeId.
    let shape = crate::object::shapes::object_shape_descriptor(obj)?;
    let keys = shape.keys as usize as *const crate::array::ArrayHeader;
    if !keys.is_null()
        && crate::object::keys_find_slot_by_bytes_resolved(keys, shape.logical_key_count, name)
            .is_some()
    {
        return None;
    }
    // The first prototype the receiver's shape pins (the same start a chain
    // proof takes).
    let start = read_holder::class_link(obj).or_else(|| {
        let (pid, link) = read_holder::admitted_link(obj)?;
        Some(if pid == crate::object::shapes::PROTO_ID_DEFAULT {
            crate::array::object_prototype_addr_if_resolved() as *const ObjectHeader
        } else {
            read_holder::next_from_word(obj, link)
        })
    })?;
    if start.is_null() {
        return None;
    }
    let ChainMethod::Data { value, holder, .. } = chain_method(start, &MethodKey::bytes(name))
    else {
        return None;
    };
    // Every hop up to and including the holder must be marked, so that its
    // structural changes and method replacements move the epoch.
    let mut hop = start;
    for _ in 0..128 {
        if !crate::object::proto_validity::object_is_marked_prototype(hop as usize) {
            return None;
        }
        if hop == holder {
            break;
        }
        hop = match next_holder(hop) {
            Ok(Some(next)) if next != hop => next,
            _ => return None,
        };
    }
    if hop != holder {
        return None;
    }
    // A function object whose body lives in the permanent image.
    if value & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return None;
    }
    let f = (value & crate::value::POINTER_MASK) as usize;
    if !crate::value::addr_class::is_above_handle_band(f) || !crate::closure::is_closure_ptr(f) {
        return None;
    }
    let info = (*(f as *const crate::closure::ClosureHeader))
        .info
        .as_ref()?;
    let permanent = crate::codegen_abi::FN_PERMANENT_IMAGE | crate::codegen_abi::FN_COMPILED_BODY;
    (info.flags & permanent == permanent && !info.code.is_null()).then_some(info)
}

/// `PERRY_CHECK_CONTRACTS=1` (decision 129 §4.5): the run-time witness of a
/// certified hit, one of the contract witnesses of `crate::entry_contracts`.
/// The hit must hold its fact for the live receiver, and the live chain must
/// still prove the certified body; otherwise it reports through the same
/// `contract_violated`. Codegen emits the call only in contract-check builds;
/// release code never references it.
///
/// # Safety
/// As [`js_method_site_certify`].
#[no_mangle]
#[inline(never)]
pub unsafe extern "C-unwind" fn js_method_site_cert_contract_check(
    recv: f64,
    method_id: i64,
    proof_word: u64,
    proof_epoch: u64,
    info: u64,
) {
    let addr = (recv.to_bits() & crate::value::POINTER_MASK) as usize;
    let obj = if miss_entry::is_site_receiver(recv) {
        ordinary_receiver(addr)
    } else {
        None
    };
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let name = crate::string::perry_string_ref_from_dispatch_id(method_id, &mut scratch)
        .map(|r| std::slice::from_raw_parts(r.ptr, r.len))
        .unwrap_or(b"?");
    let Some(obj) = obj else {
        contract_abort(
            recv,
            name,
            proof_word,
            proof_epoch,
            info,
            "receiver is not ordinary",
        );
    };
    let fact = PristineReceiverFact::Shape {
        receiver_word: receiver_word(obj),
        proof_word,
        proof_epoch,
    };
    if !fact.holds() {
        contract_abort(
            recv,
            name,
            proof_word,
            proof_epoch,
            info,
            "Shape fact does not hold",
        );
    }
    // A body-0 certificate only routes to the generic arm, which is correct
    // for any receiver; a body certificate must still be proven.
    let _no_move = crate::gc::GcSuppressScope::new();
    let proved = prove(obj, name).map_or(0, |i| i as *const _ as u64);
    if info != 0 && proved != info {
        contract_abort(
            recv,
            name,
            proof_word,
            proof_epoch,
            info,
            "live chain proves another body",
        );
    }
}

#[cold]
#[inline(never)]
unsafe fn contract_abort(
    recv: f64,
    name: &[u8],
    proof_word: u64,
    proof_epoch: u64,
    info: u64,
    why: &str,
) -> ! {
    let addr = (recv.to_bits() & crate::value::POINTER_MASK) as usize;
    let live = if miss_entry::is_site_receiver(recv) {
        ordinary_receiver(addr).map_or(0, |o| receiver_word(o))
    } else {
        0
    };
    crate::entry_contracts::contract_violated(
        "method_site_cert",
        "Shape(receiver_word, proof_word, proof_epoch)",
        "method-certificate",
        recv.to_bits(),
        &format!(
            " method={} why={why} live_word={live:#x} proof_word={proof_word:#x} \
             proof_epoch={proof_epoch} live_epoch={} body={info:#x}",
            String::from_utf8_lossy(name),
            crate::object::builtin_proto::proof_epoch(),
        ),
    )
}

#[cfg(test)]
#[path = "cert_tests.rs"]
mod tests;
