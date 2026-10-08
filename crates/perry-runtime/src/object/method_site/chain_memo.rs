//! The chain memo: a by-name method call site's memo of the prototype walk
//! that answered it (class-table retirement slice 3).
//!
//! A class instance's method call that the compiled code cannot answer
//! inline reaches the runtime by name: the method site's miss
//! ([`super::js_method_site_miss`]), a computed-key call `obj[k](...)`, or the
//! miss edge of a compiled class-method arm. The answer is a walk of the
//! receiver's prototype chain by SHAPES
//! (`native_call_method::class_holder`): every prototype before the holder
//! lacks the name, the holder's shape lists it at an inline slot. That walk
//! costs a key-list search per prototype; this memo lets a site answer a
//! repeat with one word compare per object instead.
//!
//! # What a way claims, and why it cannot go stale
//!
//! A way holds facts of ShapeIds only, each compared on every use, exactly
//! as the method site's inherited entry and the read site's holder entry
//! (`read_holder`) do:
//!
//! * the receiver's `(class_id | ShapeId)` word: the receiver lacks the name
//!   (its key list), and its [[Prototype]] is the first hop (a declared
//!   class's prototype, whose relink retires the displaced prototype's
//!   ShapeId, or the serial its shape names). An instance of a
//!   per-evaluation class names its evaluation's prototype by serial, so
//!   each evaluation's instances have their own ShapeIds and a way never
//!   answers one evaluation's receiver with another's method;
//! * each hop's word, read from the hop itself: the hop lacks the name and
//!   links to the next hop by the serial identity its shape records (a
//!   `setPrototypeOf`, key add, delete or descriptor change restamps it);
//! * the holder's word: the name is an own inline DATA slot at `slot`, and,
//!   when the shape's lane says ConstFn, the slot holds a function object of
//!   `body`. Otherwise the hit re-proves the loaded value as a compiled
//!   function, so a value overwrite without a shape change is seen.
//!
//! Nothing is keyed on a class id, an address or a name alone, and there is
//! no global invalidation word: a change to any object the way names moves
//! that object's word, and the next use misses and walks again.
//!
//! The memo is consulted before any other probe of the dispatch. A
//! computed-key site also records the key's bytes per way (the name is the
//! site's, otherwise). A method site's miss keeps no memo: its misses see
//! every kind of receiver, and a deep class chain there was measured flat
//! while the extra argument cost the plain-object misses. Ways are replaced in turn; after
//! [`CHAIN_MEMO_MAX_RECORDS`] replacements the site stops recording (it has
//! more receivers than ways) and its misses walk, as before the memo.
//!
//! # GC
//!
//! The hops are STRONG roots, rewritten when they move
//! ([`scan_chain_memo_roots_mut`]); every memo that recorded a way is
//! registered once. Recording runs on the walk's straight line, which
//! allocates nothing.
//!
//! # Agents
//!
//! Like every method site: once a worker exists no memo is read, recorded or
//! traced (the hops belong to the primary heap).

use super::WORKER_AGENTS_EXIST;
use crate::object::ObjectHeader;
use std::sync::atomic::Ordering;

/// Objects a way names: the prototypes from the receiver's [[Prototype]] to
/// the holder, inclusive.
pub const CHAIN_MEMO_MAX_HOPS: usize = 8;
/// Ways per site.
const CHAIN_MEMO_WAYS: usize = 2;
/// Longest computed key a way records.
const CHAIN_MEMO_KEY_MAX: usize = 32;
/// Replacements after which a site stops recording.
pub const CHAIN_MEMO_MAX_RECORDS: u32 = 16;

#[repr(C)]
#[derive(Clone, Copy)]
struct Way {
    /// The receiver's `(class_id | ShapeId << 32)` header word.
    recv: u64,
    /// Hops named (holder included); 0 = an empty way.
    depth: u32,
    /// The holder's inline slot.
    slot: u32,
    /// The `JsFunctionInfo` the holder shape's ConstFn lane names for the
    /// slot, or 0.
    body: usize,
    /// A computed site's key length (its bytes in `key`).
    key_len: u32,
    _pad: u32,
    key: [u8; CHAIN_MEMO_KEY_MAX],
    /// Hop addresses, receiver's [[Prototype]] first (STRONG roots).
    hops: [usize; CHAIN_MEMO_MAX_HOPS],
    /// Each hop's header word when recorded.
    words: [u64; CHAIN_MEMO_MAX_HOPS],
}

/// One site's memo, allocated in the IC arena on its first record. All-zero
/// is the empty memo.
#[repr(C)]
pub struct ChainMemo {
    ways: [Way; CHAIN_MEMO_WAYS],
    records: u32,
    registered: u32,
}

/// The emitted `@perry_cmemo_N = private global ptr null` of a computed-key
/// call site, or the word after a compiled class-method site's learned word.
pub type ChainMemoSlot = *mut ChainMemo;

/// How a by-name call names its site's memo: 0 (none), or a memo slot's
/// address, tagged [`MEMO_KEYED`] when the site's key is computed (each way
/// records the key's bytes). Every slot is 8-byte aligned, so the low bits
/// are free.
pub(crate) type MemoRef = usize;
const MEMO_KEYED: usize = 1;

/// The memo slot `memo` names.
#[inline]
fn memo_slot(memo: MemoRef) -> *mut ChainMemoSlot {
    (memo & !MEMO_KEYED) as *mut ChainMemoSlot
}

/// The [`MemoRef`] of a memo slot.
#[inline]
pub(crate) fn memo_ref_slot(slot: *mut ChainMemoSlot, keyed: bool) -> MemoRef {
    if slot.is_null() {
        return 0;
    }
    slot as usize | if keyed { MEMO_KEYED } else { 0 }
}

/// Every memo that recorded a way, for the primary agent's root scan.
static CHAIN_MEMOS: std::sync::Mutex<Vec<usize>> = std::sync::Mutex::new(Vec::new());

per_test_global! {
    static MEMO_RECORDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
}

/// Ways recorded (diagnostic; tests prove a path ran).
pub fn chain_memo_records() -> u64 {
    MEMO_RECORDS.load(Ordering::Relaxed)
}

#[inline(always)]
unsafe fn header_word(addr: usize) -> u64 {
    std::ptr::read(addr as *const u64)
}

/// The value a way of `memo` names for `object`, and the body its holder's
/// shape names for the slot: `object` is a heap ordinary object whose
/// metadata record, if any, holds storage only, and its header word, every
/// hop's word and (for a keyed site) `name` equal a way's. `None` otherwise.
/// Loads only.
///
/// # Safety
/// `memo` is 0 or names a live memo slot.
#[inline]
pub(crate) unsafe fn memo_lookup_value(
    memo: MemoRef,
    object: f64,
    name: &[u8],
) -> Option<(u64, Option<&'static crate::closure::JsFunctionInfo>)> {
    let memo_ptr = crate::object::pic_slot_peek(memo_slot(memo));
    if memo_ptr.is_null() || WORKER_AGENTS_EXIST.load(Ordering::Relaxed) != 0 {
        return None;
    }
    let bits = object.to_bits();
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return None;
    }
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    if !crate::value::addr_class::is_above_handle_band(addr) {
        return None;
    }
    let header = crate::value::addr_class::try_read_gc_header(addr)?;
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return None;
    }
    // The receiver predicate's per-object part its word does not carry: a
    // metadata record only for storage (no flags, no dictionary keys). A
    // recorded [[Prototype]] is the one its ShapeId names (a way is recorded
    // only for a receiver whose shape states it), and a per-evaluation
    // class's private brand is a fact of that prototype.
    let meta = (*(addr as *const ObjectHeader)).meta;
    if !meta.is_null() && ((*meta).flags != 0 || (*meta).dictionary_keys != 0) {
        return None;
    }
    ways_lookup(&*memo_ptr, memo & MEMO_KEYED != 0, addr, name)
}

#[inline]
unsafe fn ways_lookup(
    memo: &ChainMemo,
    keyed: bool,
    recv: usize,
    name: &[u8],
) -> Option<(u64, Option<&'static crate::closure::JsFunctionInfo>)> {
    let word = header_word(recv);
    'ways: for way in memo.ways.iter() {
        if way.recv != word || way.depth == 0 {
            continue;
        }
        if keyed && (way.key_len as usize != name.len() || way.key[..name.len()] != *name) {
            continue;
        }
        let depth = way.depth as usize;
        for i in 0..depth {
            if header_word(way.hops[i]) != way.words[i] {
                continue 'ways;
            }
        }
        let holder = way.hops[depth - 1];
        let value = std::ptr::read(
            (holder + std::mem::size_of::<ObjectHeader>() + way.slot as usize * 8) as *const u64,
        );
        let body = (way.body as *const crate::closure::JsFunctionInfo).as_ref();
        return Some((value, body));
    }
    None
}

/// A hop a way may name: an ordinary, shaped object at a stable heap address
/// whose shape answers own string-keyed lookups.
#[inline]
unsafe fn hop_admitted(addr: usize) -> bool {
    if !crate::value::addr_class::is_above_handle_band(addr)
        || !super::address_is_prime_stable(addr)
    {
        return false;
    }
    let Some(header) = crate::value::addr_class::try_read_gc_header(addr) else {
        return false;
    };
    let obj = addr as *const ObjectHeader;
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || header._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0
        || crate::closure::is_closure_ptr(addr)
        || (*obj).class_id == crate::object::NATIVE_MODULE_CLASS_ID
        || crate::object::dictionary::is_dictionary(obj)
        || crate::object::shapes::object_shape_stamp(obj) == 0
    {
        return false;
    }
    let meta = (*obj).meta;
    meta.is_null()
        || ((*meta).elements == 0
            && (*meta).dictionary_keys == 0
            && (*meta).flags & crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER == 0)
}

/// The next hop after `hop`, when `hop`'s shape pins it: a serial identity
/// (plain or MIXED with its class), or the realm's `%Object.prototype%`.
#[inline]
unsafe fn pinned_next(hop: *const ObjectHeader) -> Option<*const ObjectHeader> {
    use crate::object::shapes::{PROTO_ID_DEFAULT, PROTO_ID_NULL};
    let (pid, word) = super::read_holder::admitted_link(hop)?;
    let next = if pid == PROTO_ID_DEFAULT {
        crate::array::object_prototype_addr_if_resolved() as *const ObjectHeader
    } else if pid == PROTO_ID_NULL {
        return None;
    } else {
        super::read_holder::next_from_word(hop, word)
    };
    (!next.is_null() && next != hop).then_some(next)
}

/// Is `name` an array index, which a hop's element storage (not its shape)
/// would answer?
#[inline]
fn name_is_index(name: &[u8]) -> bool {
    !name.is_empty() && name.len() <= 10 && name.iter().all(u8::is_ascii_digit)
}

/// Record the walk that answered `name` for `recv`: its chain from `start`
/// (the receiver's [[Prototype]]) to `holder`, where the name is an own
/// inline data slot `slot` whose ConstFn lane (if any) names `body`. Every
/// fact is re-read here from shapes; any object whose shape does not pin it
/// refuses, and the site keeps walking. Allocation-free on the GC heap.
///
/// # Safety
/// `memo` is 0 or names a live memo slot; the objects are live, and no
/// collection runs between the walk and this call.
pub(crate) unsafe fn memo_record(
    memo: MemoRef,
    recv: *const ObjectHeader,
    start: *const ObjectHeader,
    holder: *const ObjectHeader,
    holder_slot: u32,
    body: Option<&'static crate::closure::JsFunctionInfo>,
    name: &[u8],
) {
    if memo == 0 || WORKER_AGENTS_EXIST.load(Ordering::Relaxed) != 0 || name_is_index(name) {
        return;
    }
    let keyed = memo & MEMO_KEYED != 0;
    if keyed && name.len() > CHAIN_MEMO_KEY_MAX {
        return;
    }
    let existing = crate::object::pic_slot_peek(memo_slot(memo));
    if !existing.is_null() && (*existing).records >= CHAIN_MEMO_MAX_RECORDS {
        return;
    }
    // The receiver's word pins its [[Prototype]] only as its shape states it.
    if super::read_holder::class_link(recv) != Some(start) {
        return;
    }
    let mut way = Way {
        recv: header_word(recv as usize),
        depth: 0,
        slot: holder_slot,
        body: body.map_or(0, |b| b as *const _ as usize),
        key_len: 0,
        _pad: 0,
        key: [0; CHAIN_MEMO_KEY_MAX],
        hops: [0; CHAIN_MEMO_MAX_HOPS],
        words: [0; CHAIN_MEMO_MAX_HOPS],
    };
    if keyed {
        way.key_len = name.len() as u32;
        way.key[..name.len()].copy_from_slice(name);
    }
    let mut hop = start;
    loop {
        let depth = way.depth as usize;
        if depth == CHAIN_MEMO_MAX_HOPS || !hop_admitted(hop as usize) {
            return;
        }
        way.hops[depth] = hop as usize;
        way.words[depth] = header_word(hop as usize);
        way.depth += 1;
        if hop == holder {
            break;
        }
        let Some(next) = pinned_next(hop) else {
            return;
        };
        hop = next;
    }
    let Some(record) = crate::object::shapes::object_shape_record(holder) else {
        return;
    };
    if holder_slot >= record.live_inline_slot_count() {
        return;
    }
    let Ok(mut memos) = CHAIN_MEMOS.lock() else {
        return;
    };
    let memo = crate::object::pic_slot_resolve(memo_slot(memo));
    if memo.is_null() {
        return;
    }
    let memo = &mut *memo;
    let idx = memo
        .ways
        .iter()
        .position(|w| w.depth == 0)
        .unwrap_or(memo.records as usize % CHAIN_MEMO_WAYS);
    memo.records += 1;
    if memo.registered == 0 {
        memo.registered = 1;
        memos.push(memo as *mut ChainMemo as usize);
    }
    memo.ways[idx] = way;
    MEMO_RECORDS.fetch_add(1, Ordering::Relaxed);
}

/// Root scan: before workers exist, every recorded hop is marked and
/// rewritten when it moves. After the sticky worker gate no memo is read, so
/// the scan stops and otherwise-dead hops can collect.
pub(crate) fn scan_chain_memo_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    if crate::agent::current_agent() != crate::agent::PRIMARY_AGENT
        || WORKER_AGENTS_EXIST.load(Ordering::SeqCst) != 0
    {
        return;
    }
    if let Ok(memos) = CHAIN_MEMOS.lock() {
        for &memo in memos.iter() {
            // SAFETY: registered memos are IC-arena allocations, never freed.
            for way in unsafe { (*(memo as *mut ChainMemo)).ways.iter_mut() } {
                for i in 0..way.depth as usize {
                    visitor.visit_tagged_usize_slot(&mut way.hops[i], crate::value::POINTER_TAG);
                }
            }
        }
    }
}
