//! ChainDesc glue (design /root/DYNP/chain1/DESIGN-CHAIN.md §2-§5): the node of
//! a marked prototype (created by the transition that makes it a prototype;
//! its meta record's owned-state bag names the node's desc), the own-key lister the pre-order rebuild uses, the publication hooks
//! and the read leaf that replaces the hop walk.
//!
//! A node's own keys are listed from its layout record's key list with the
//! same probe the hop walk used (`list_position`), so a desc entry names the
//! holder and the slot the walk would have read. A node whose own keys are not
//! exactly its list (overflow shadow bit, a TEXT key, a record not derived
//! yet, a declined probe word, a deferred namespace, an exotic read receiver,
//! an unlisted builtin kind) is a boundary: a miss at or past it runs the
//! complete walk from it, which is the existing exact path.
use super::*;
use crate::object::chain_desc::{self as cd, act, ChainDesc, ChainNode, Found, OwnKey};

const SLOT: u32 = crate::object::owned_state_bag::CHAIN_NODE;
const PTR_TAG: u64 = 0x7FFD_0000_0000_0000;
/// The owned-state slot word of a prototype: its desc's address in the
/// internal band.
const NODE_TAG: u64 = crate::value::INTERNAL_WORD_TAG;

thread_local! {
    static ALL: std::cell::Cell<*mut ChainNode> = const { std::cell::Cell::new(std::ptr::null_mut()) };
    /// The nodes whose [[Prototype]] is an object without a node yet
    /// (`ChainNode::waiting`); intrusive through `wait_next`/`wait_prev`.
    static WAIT: std::cell::Cell<*mut ChainNode> = const { std::cell::Cell::new(std::ptr::null_mut()) };
}

/// Put `n` on the waiting list or take it off.
unsafe fn set_waiting(n: *mut ChainNode, on: bool) {
    if (*n).waiting == on {
        return;
    }
    (*n).waiting = on;
    if on {
        let head = WAIT.with(|w| w.get());
        (*n).wait_prev = std::ptr::null_mut();
        (*n).wait_next = head;
        if !head.is_null() {
            (*head).wait_prev = n;
        }
        WAIT.with(|w| w.set(n));
    } else {
        let (p, q) = ((*n).wait_prev, (*n).wait_next);
        if p.is_null() {
            WAIT.with(|w| w.set(q));
        } else {
            (*p).wait_next = q;
        }
        if !q.is_null() {
            (*q).wait_prev = p;
        }
        (*n).wait_prev = std::ptr::null_mut();
        (*n).wait_next = std::ptr::null_mut();
    }
}

#[inline(always)]
pub(crate) unsafe fn obj_of(n: *const ChainNode) -> *const ObjectHeader {
    ((*n).obj & POINTER_MASK) as *const ObjectHeader
}

/// The owned-state slot of `obj` that names its desc, or null (no meta, or
/// a bag that does not reach the slot). `obj` is a `GC_TYPE_OBJECT`.
#[inline(always)]
unsafe fn slot_of(obj: *const ObjectHeader) -> *mut u64 {
    // A1_UNPROVEN: record-first, as `link_of` reads a prototype's meta.
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _));
    if meta.is_null() {
        return std::ptr::null_mut();
    }
    let bag = (*meta).owned_state as usize as *mut crate::array::ArrayHeader;
    if bag.is_null() || (*bag).length <= SLOT {
        return std::ptr::null_mut();
    }
    // A bag is exact (`owned_state_bag::ELEMENTS_WORDS`): the slot sits at a
    // fixed offset, so its load does not wait for the bag's header words.
    crate::object::owned_state_bag::slot_word(bag, SLOT)
}

/// The desc of `obj`'s node, or null (not a marked prototype, or no node).
/// The read leaves start here: the slot names the desc, not the node.
#[inline(always)]
pub(crate) unsafe fn desc_of(obj: *const ObjectHeader) -> *const ChainDesc {
    // Exotic holders are boundaries: a link may name any heap kind (an
    // Array.prototype array, a Map, a DataView), and only an object has a
    // meta word, so only an object has a node. This is the holder's own type
    // dispatch, the same header byte the walk's kind split reads; a
    // non-object returns no node and the complete walk answers from it.
    if crate::object::header::object_type(obj as *mut ObjectHeader) != crate::gc::GC_TYPE_OBJECT {
        return std::ptr::null();
    }
    let sp = slot_of(obj);
    if sp.is_null() {
        return std::ptr::null();
    }
    // The bag is the meta's own runtime array (`owned_state_bag`), dense and
    // never seen by the program, and the length check above bounds the slot:
    // its element word is the desc, read as `accessor_pair` reads its own
    // array. The generic element entry would re-classify a raw pointer on
    // every absent read.
    // The slot word is the desc's address in the runtime-internal band
    // (`NODE_TAG | addr`, written only at a publication: `publish`) or the
    // bag's fill word. The band is never a JS value and never
    // pointer-bearing, so the GC's layout trace (the bag is a POINTER_FREE
    // array) reads no pointer.
    let bits = *sp;
    if bits >> 48 != NODE_TAG >> 48 {
        return std::ptr::null();
    }
    (bits & POINTER_MASK) as *const ChainDesc
}

/// The node of `obj`, or null (not a marked prototype, or no node).
#[inline(always)]
pub(crate) unsafe fn node_of(obj: *const ObjectHeader) -> *mut ChainNode {
    let d = desc_of(obj);
    if d.is_null() {
        std::ptr::null_mut()
    } else {
        (*d).node
    }
}

/// A publication: `d` is `n`'s fresh desc; its object's slot names it from
/// now on (before the old desc is freed). Eager, at the mutation; reads
/// never write it.
unsafe fn publish(n: *mut ChainNode, d: *const ChainDesc) {
    let sp = slot_of(obj_of(n));
    if !sp.is_null() {
        // GC_STORE_AUDIT(POINTER_FREE): an internal-band word in the
        // POINTER_FREE owned-state bag, never a heap reference.
        *sp = NODE_TAG | d as u64;
    }
}

/// Establishment: `obj` just became a prototype (the IS_PROTOTYPE 0 -> 1
/// transition, at the place that performs it; a repeat mark runs no chain
/// code). Gives it a node, links it under its prototype's node and builds
/// its desc (ancestors are marked first by the link funnel, so they already
/// have nodes). An object whose establishment found no slot stays a
/// boundary: a later mark does not retry it (exact; the reads walk).
pub(crate) unsafe fn establish(obj: usize) {
    let o = obj as *const ObjectHeader;
    // A marked exotic holder (an Array subclass prototype chain reaches
    // Array.prototype, an array) gets no node: it stays a boundary.
    if obj == 0
        || crate::object::header::object_type(obj as *mut ObjectHeader) != crate::gc::GC_TYPE_OBJECT
        || !desc_of(o).is_null()
    {
        return;
    }
    let _no_gc = crate::gc::GcSuppressScope::new();
    // The bag's own store sizes it; the slot word takes the desc's tagged
    // address at the node's first publication (the rebuild below).
    crate::object::owned_state_bag::store(obj, SLOT, 0.0);
    if slot_of(o).is_null() {
        // No bag reaches the slot on this holder: no node (reads walk).
        return;
    }
    let n = cd::new_node(PTR_TAG | obj as u64, false);
    let head = ALL.with(|a| a.get());
    (*n).all_next = head;
    if !head.is_null() {
        (*head).all_prev = n;
    }
    ALL.with(|a| a.set(n));
    relink(n);
    // Adoption: a prototype marked before this one (an intrinsic sealed
    // before its own [[Prototype]] was marked) waits open above it; link it
    // here (its parent is `n`, whose slot names no desc until the rebuild
    // publishes one), then one pre-order rebuild covers this node and the
    // adopted.
    let mut m = WAIT.with(|w| w.get());
    while !m.is_null() {
        let next = (*m).wait_next;
        if m != n && matches!(link(m), Next::Object(p) if p == o) {
            relink_to(m, o, n);
        }
        m = next;
    }
    rebuild(n);
}

/// Rebuild `n`'s subtree, publishing each fresh desc.
unsafe fn rebuild(n: *mut ChainNode) {
    cd::rebuild_subtree_publishing(
        n,
        |n, out| unsafe { own_keys(n, out) },
        |n, d| unsafe { publish(n, d) },
    );
}

/// A structural mutation of `obj` (key add/delete, attributes, overflow,
/// [[Prototype]] relink, lazy population) was published: relink its node and
/// rebuild its subtree, ancestor before descendant, before control returns.
pub(crate) unsafe fn on_mutation(obj: usize) {
    // The relink funnel reports every relinked receiver's pointer-tag
    // payload: heap values (collections, DataView, arrays) and registry
    // handles. A payload in the handle band is a handle id by the band map's
    // definition (`value::addr_class`, the classification every handle-aware
    // path makes) and has no header; above it the payload is a heap
    // allocation whose own header type says whether it is an object, the
    // only kind with a meta word and so a node.
    if crate::value::addr_class::is_handle_band(obj)
        || crate::object::header::object_type(obj as *mut ObjectHeader) != crate::gc::GC_TYPE_OBJECT
    {
        return;
    }
    let n = node_of(obj as *const ObjectHeader);
    if n.is_null() {
        return;
    }
    let _no_gc = crate::gc::GcSuppressScope::new();
    relink(n);
    rebuild(n);
}

/// The seal of intrinsic `obj` was just stored (`builtin_proto` seal flag).
/// The seal makes a callable or typed-array-prototype record's list its
/// exact own keys (`inherited::listed`), and it is stored after the mark (the
/// seal's write tracking must not see the mark's own publications), so a
/// node the mark established is exotic only for want of it: rebuild it now,
/// the seal being the publication that makes the list authoritative.
pub(crate) unsafe fn on_seal(obj: usize) {
    let n = node_of(obj as *const ObjectHeader);
    if !n.is_null() && (*n).exotic {
        on_mutation(obj);
    }
}

/// Put `n` under the node of its current [[Prototype]]; a link the probe does
/// not follow, or a prototype without a node, leaves it open above.
unsafe fn relink(n: *mut ChainNode) {
    relink_to(n, std::ptr::null(), std::ptr::null_mut());
}

/// [`relink`], with `known` the node of object `known_obj` (an
/// establishment whose node has no published desc yet).
unsafe fn relink_to(n: *mut ChainNode, known_obj: *const ObjectHeader, known: *mut ChainNode) {
    // An object prototype without a node may be marked later: wait for it.
    // An exotic one never gets a node (`establish`), nor does an unknown link.
    let (parent, open, wait) = match link(n) {
        Next::Object(p) => {
            let pn = if !known_obj.is_null() && p == known_obj {
                known
            } else {
                node_of(p)
            };
            let wait = pn.is_null()
                && crate::object::header::object_type(p as *mut ObjectHeader)
                    == crate::gc::GC_TYPE_OBJECT;
            (pn, pn.is_null(), wait)
        }
        Next::End => (std::ptr::null_mut(), false, false),
        Next::Unknown => (std::ptr::null_mut(), true, false),
    };
    set_waiting(n, wait);
    if (*n).parent != parent {
        cd::unlink(n);
        // A cycle cannot be linked (setPrototypeOf refuses one), but never
        // put a node under its own subtree.
        let mut a = parent;
        while !a.is_null() {
            if a == n {
                (*n).open_above = true;
                return;
            }
            a = (*a).parent;
        }
        cd::link(n, parent);
    }
    (*n).open_above = open;
}

/// The [[Prototype]] link of node `n`'s object as the probe follows it.
unsafe fn link(n: *mut ChainNode) -> Next {
    let dir = super::super::ordinary_dir_addr();
    let o = obj_of(n);
    let r = record_of(dir, LayoutWord((*o).parent_class_id));
    if r.is_null() {
        return Next::Unknown;
    }
    inherited::link_of(dir, o, r)
}

/// The own keys of node `n` exactly as the hop walk reads them, or none and
/// `exotic` set when its own keys are not exactly its list.
unsafe fn own_keys(n: *mut ChainNode, out: &mut Vec<OwnKey>) {
    own_keys_why(n, out);
}

/// [`own_keys`] with the reason a node is exotic: 0 when its own keys are
/// exactly its list, else the number of the check that gave up.
unsafe fn own_keys_why(n: *mut ChainNode, out: &mut Vec<OwnKey>) -> u8 {
    (*n).exotic = true;
    let dir = super::super::ordinary_dir_addr();
    let o = obj_of(n);
    if (*o).class_id == crate::object::deferred_namespace::DEFERRED_NAMESPACE_CLASS_ID {
        return 1;
    }
    let w = (*o).parent_class_id;
    if w & crate::object::overflow::SHADOW_BIT != 0 {
        return 2;
    }
    let r = record_of(dir, LayoutWord(w));
    if r.is_null() {
        return 3;
    }
    match (*r).object_kind() {
        super::super::ShapeObjectKind::Ordinary => {}
        kind if inherited::listed(o, kind) => {}
        _ => return 8,
    }
    if crate::object::proto_validity::object_is_exotic_read_receiver(o as usize) {
        return 4;
    }
    let pw = probe_word(r);
    if pw & (PROBE_READY | PROBE_DECLINE | PROBE_ATOMS) != PROBE_READY | PROBE_ATOMS {
        return 5;
    }
    let count = (*r).logical_key_count;
    let list = if count == 0 {
        std::ptr::null()
    } else {
        crate::array::array_elements_ptr((*r).keys as usize as *const _) as *const u64
    };
    let start = out.len();
    for i in 0..count {
        let kb = *list.add(i as usize);
        let Some(hash) = key_word_hash(kb) else {
            out.truncate(start);
            return 6;
        };
        match list_position(r, pw, kb, hash) {
            Ok((pos, load)) => out.push(OwnKey {
                key_bits: kb,
                hash,
                loc: pos,
                load,
                act: if load == NO_LOAD && attributed_at(r, pos) {
                    act::ACCESSOR
                } else {
                    act::DATA_INLINE
                },
                enumerable: kb & TAG_MASK == STRING_TAG
                    && ((*r).summary() & crate::object::key_attrs::SUMMARY_NON_ENUMERABLE == 0
                        || crate::object::key_attrs::keys_entry(
                            (*r).keys as usize as *const crate::array::ArrayHeader,
                            pos,
                        ) & crate::object::key_attrs::ENTRY_NON_ENUMERABLE
                            == 0),
            }),
            Err(_) => {
                out.truncate(start);
                return 7;
            }
        }
    }
    (*n).exotic = false;
    0
}

/// The read leaf: `next` is the receiver's [[Prototype]] (the link of
/// `cur`) and `d` its node's desc, the receiver's own list and overflow
/// proved the key absent. One exact lookup in the desc; every answer the
/// desc does not settle continues in the hop loop at the hop it names, with
/// `cur` for the first hop (its decline is the loop's own), so no chain
/// reads slower than the loop.
#[inline(always)]
pub(super) unsafe fn read<const ABSENT: bool>(
    dir: *const u8,
    cur: *const ObjectHeader,
    next: *const ObjectHeader,
    d: *const ChainDesc,
    key_bits: u64,
    hash: u64,
) -> f64 {
    match ChainDesc::lookup(d, key_bits, hash) {
        Found::Hit(e) => {
            // The entry names the holder's inline slot (the desc is rebuilt
            // at every publication of the holder's layout), so a data hit
            // reads it without the holder's record. A hit on the first hop
            // reads `next` itself, the object the desc was reached from.
            if e.load != NO_LOAD {
                let h = if e.holder == (*d).node {
                    next
                } else {
                    obj_of(e.holder)
                };
                let v = *((h as *const u8)
                    .add(std::mem::size_of::<ObjectHeader>() + e.load as usize * 8)
                    as *const f64);
                if v.to_bits() != TAG_HOLE {
                    return v;
                }
            }
            read_hit_rest::<ABSENT>(dir, cur, (*d).node, e, key_bits, hash)
        }
        Found::Text(_) => read_at::<ABSENT>(dir, cur, (*d).node, (*d).node, key_bits, hash),
        Found::Miss(b) if b.is_null() => inherited::chain_absent::<ABSENT>(),
        Found::Miss(b) => read_at::<ABSENT>(dir, cur, (*d).node, b, key_bits, hash),
    }
}

/// [`read`]'s hit that no inline slot answered: a position the record reads
/// without a load, an accessor, or a hole (the walk continues at the holder).
#[inline(never)]
unsafe fn read_hit_rest<const ABSENT: bool>(
    dir: *const u8,
    cur: *const ObjectHeader,
    node: *mut ChainNode,
    e: cd::ChainEntry,
    key_bits: u64,
    hash: u64,
) -> f64 {
    let h = obj_of(e.holder);
    let r = record_of(dir, LayoutWord((*h).parent_class_id));
    if e.load == NO_LOAD {
        let v = value_at(h, r, e.loc);
        if v.to_bits() != TAG_HOLE {
            return v;
        }
    }
    if e.act == act::ACCESSOR {
        return accessor_answer_at(h, r, e.loc);
    }
    read_at::<ABSENT>(dir, cur, node, e.holder, key_bits, hash)
}

/// [`read`]'s continuation in the hop loop at `hop`, the hop the desc did
/// not settle (`cur` for the first hop, whose decline is the loop's own).
/// Out of line, so the desc answers carry none of its arguments.
#[cold]
#[inline(never)]
unsafe fn read_at<const ABSENT: bool>(
    dir: *const u8,
    cur: *const ObjectHeader,
    node: *mut ChainNode,
    hop: *mut ChainNode,
    key_bits: u64,
    hash: u64,
) -> f64 {
    let c = if hop == node { cur } else { obj_of(node) };
    inherited::chain_loop::<ABSENT>(dir, c, obj_of(hop), key_bits, hash)
}

/// The `in` leaf: `obj` is a prototype hop of the receiver and `d` its
/// node's desc, the hops below it proved the exact string key absent. A hit is answered by the holder's
/// own hop (a hole slot or a private entry continue or decline there, as in
/// the walk); a miss is absence up to the null link, or the walk from the
/// boundary it names; a TEXT entry walks from `node`.
#[inline(always)]
pub(super) unsafe fn has(
    dir: *const u8,
    obj: *const ObjectHeader,
    d: *const ChainDesc,
    key_bits: u64,
    hash: u64,
    absent: i32,
) -> i32 {
    match ChainDesc::lookup(d, key_bits, hash) {
        Found::Hit(e) => {
            let h = if e.holder == (*d).node {
                obj
            } else {
                obj_of(e.holder)
            };
            super::has::probe_has_walk_from(dir, h, key_bits, hash)
        }
        Found::Text(_) => super::has::probe_has_walk_from(dir, obj, key_bits, hash),
        Found::Miss(b) if b.is_null() => absent,
        Found::Miss(b) => super::has::probe_has_walk_from(dir, obj_of(b), key_bits, hash),
    }
}

/// GC: every node's `obj` word, rewritten like the shape identity words.
pub(crate) unsafe fn for_each_node_obj(mut f: impl FnMut(*mut u64)) {
    let mut n = ALL.with(|a| a.get());
    while !n.is_null() {
        f(std::ptr::addr_of_mut!((*n).obj));
        n = (*n).all_next;
    }
}

/// GC: free the nodes (and descs) of dead prototypes.
pub(crate) unsafe fn prune(is_dead_owner: &dyn Fn(usize) -> bool) {
    let mut n = ALL.with(|a| a.get());
    while !n.is_null() {
        let next = (*n).all_next;
        if is_dead_owner(obj_of(n) as usize) {
            set_waiting(n, false);
            let (p, q) = ((*n).all_prev, (*n).all_next);
            if p.is_null() {
                ALL.with(|a| a.set(q));
            } else {
                (*p).all_next = q;
            }
            if !q.is_null() {
                (*q).all_prev = p;
            }
            let mut c = (*n).first_child;
            while !c.is_null() {
                (*c).open_above = true;
                c = (*c).next_sibling;
            }
            cd::free_node(n);
        }
        n = next;
    }
}

/// for-in past the receiver: `proto` (the receiver's [[Prototype]], already
/// read by the walk) has a node whose desc is complete to the null prototype
/// and holds no enumerable string key on any level, so the prototype levels
/// emit nothing. Any other answer keeps the per-level walk.
pub(crate) unsafe fn for_in_proves_empty(proto: f64) -> bool {
    let v = crate::value::JSValue::from_bits(proto.to_bits());
    if !v.is_pointer() {
        return false;
    }
    let o = v.as_pointer::<ObjectHeader>();
    let h = crate::gc::header_from_trusted_user_ptr(o.cast());
    if (*h).obj_type != crate::gc::GC_TYPE_OBJECT
        || (*h).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return false;
    }
    let d = desc_of(o);
    !d.is_null() && (*d).boundary.is_null() && (*d).enum_count == 0
}

/// Test view: the node's own keys as a fresh rebuild lists them (bits,
/// hash), and the reason it would be exotic; the node is left unchanged.
#[cfg(test)]
pub(crate) unsafe fn test_fresh_own_keys(obj: *const ObjectHeader) -> (u8, Vec<(u64, u64)>) {
    let n = node_of(obj);
    assert!(!n.is_null(), "no chain node");
    let ex = (*n).exotic;
    let mut out = Vec::new();
    let why = own_keys_why(n, &mut out);
    (*n).exotic = ex;
    (why, out.iter().map(|k| (k.key_bits, k.hash)).collect())
}

/// Test view: does the node's desc answer `key` as its own key?
#[cfg(test)]
pub(crate) unsafe fn test_desc_holds(obj: *const ObjectHeader, key_bits: u64, hash: u64) -> bool {
    let n = node_of(obj);
    !n.is_null()
        && matches!(ChainDesc::lookup((*n).desc, key_bits, hash),
            Found::Hit(e) if e.holder as usize == n as usize)
}

/// Test view: the node of `obj` as the live structure holds it, unchanged:
/// (exotic, node address, parent address, desc boundary address).
#[cfg(test)]
pub(crate) unsafe fn test_node_view(
    obj: *const ObjectHeader,
) -> Option<(bool, usize, usize, usize)> {
    let n = node_of(obj);
    if n.is_null() {
        return None;
    }
    let d = (*n).desc;
    let b = if d.is_null() {
        usize::MAX
    } else {
        (*d).boundary as usize
    };
    Some(((*n).exotic, n as usize, (*n).parent as usize, b))
}

/// Test view: the holder node address the node's desc answers `key` with,
/// or 0 when it answers no hit.
#[cfg(test)]
pub(crate) unsafe fn test_desc_holder(obj: *const ObjectHeader, key_bits: u64, hash: u64) -> usize {
    let n = node_of(obj);
    if n.is_null() {
        return 0;
    }
    match ChainDesc::lookup((*n).desc, key_bits, hash) {
        Found::Hit(e) => e.holder as usize,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    fn key(text: &str) -> *const crate::StringHeader {
        crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32)
    }

    /// The node comes with the 0 -> 1 transition; a repeat mark (`new F`
    /// re-marks F.prototype) leaves the node and the ALL list unchanged.
    #[test]
    fn a_repeat_mark_runs_no_chain_code() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _scope = crate::gc::RuntimeHandleScope::new();
        let _ = crate::object::js_get_global_this();
        unsafe {
            let o = crate::object::js_object_alloc(0, 2);
            crate::object::js_object_set_field_by_name(o, key("ra"), 1.0);
            crate::object::proto_validity::mark_object_as_prototype(o as usize);
            let (_, n1, _, _) = super::test_node_view(o).expect("no node after the first mark");
            let head = super::ALL.with(|a| a.get());
            assert_eq!(
                head as usize, n1,
                "the transition's node heads the ALL list"
            );
            crate::object::proto_validity::mark_object_as_prototype(o as usize);
            crate::object::proto_validity::mark_object_as_prototype_with_serial(
                o as usize,
                crate::object::proto_validity::next_prototype_serial(),
            );
            let (_, n2, _, _) = super::test_node_view(o).expect("the repeat mark dropped the node");
            assert_eq!(n2, n1, "a repeat mark replaced the node");
            assert_eq!(
                super::ALL.with(|a| a.get()),
                head,
                "a repeat mark touched the ALL list"
            );
        }
    }

    /// The slot names the desc: a key add on a marked prototype publishes a
    /// fresh desc, the slot names it, and it names the node back.
    #[test]
    fn the_slot_names_the_rebuilt_desc() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _scope = crate::gc::RuntimeHandleScope::new();
        let _ = crate::object::js_get_global_this();
        unsafe {
            let o = crate::object::js_object_alloc(0, 2);
            crate::object::js_object_set_field_by_name(o, key("sa"), 1.0);
            crate::object::proto_validity::mark_object_as_prototype(o as usize);
            let n = super::node_of(o);
            assert!(!n.is_null(), "no node");
            let d1 = super::desc_of(o);
            assert_eq!(d1, (*n).desc, "the slot does not name the node's desc");
            assert_eq!((*d1).node, n, "the desc does not name its node");
            crate::object::js_object_set_field_by_name(o, key("sb"), 2.0);
            let d2 = super::desc_of(o);
            assert_ne!(d2, d1, "a key add published no fresh desc");
            assert_eq!(d2, (*n).desc, "the slot names a stale desc");
            assert_eq!((*d2).node, n, "the fresh desc does not name its node");
            let (why, keys) = super::test_fresh_own_keys(o);
            if why == 0 {
                assert_eq!(keys.len(), 2, "own keys {keys:?}");
                for &(k, h) in &keys {
                    assert!(super::test_desc_holds(o, k, h), "desc lacks own key {k:#x}");
                }
            }
        }
    }

    fn intrinsic(name: &str) -> *const super::ObjectHeader {
        let v = crate::object::global_this::builtin_prototype_value(name);
        (v.to_bits() & crate::value::POINTER_MASK) as usize as *const _
    }

    /// fn__proto_own: once the realm is sealed, %Function.prototype%'s LIVE
    /// node (no manual mark, no fresh re-list) is listed, linked under
    /// %Object.prototype%'s node, complete to the null link, and answers every
    /// %Object.prototype% key it does not own from %Object.prototype%.
    #[test]
    fn sealed_function_prototype_desc_is_listed() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _scope = crate::gc::RuntimeHandleScope::new();
        let _ = crate::object::js_get_global_this();
        unsafe {
            let (op, fp) = (intrinsic("Object"), intrinsic("Function"));
            let (o_ex, o_node, _, o_bound) =
                super::test_node_view(op).expect("Object.prototype has no node");
            assert!(!o_ex, "Object.prototype node is exotic");
            assert_eq!(o_bound, 0, "Object.prototype desc has a boundary");
            let (f_ex, f_node, f_parent, f_bound) =
                super::test_node_view(fp).expect("Function.prototype has no node");
            assert!(!f_ex, "Function.prototype node is exotic");
            assert_eq!(
                f_parent, o_node,
                "Function.prototype not under Object.prototype"
            );
            assert_eq!(f_bound, 0, "Function.prototype desc has a boundary");
            // Read-only views of the keys: the live descs are what is tested.
            let (_, fkeys) = super::test_fresh_own_keys(fp);
            let (_, okeys) = super::test_fresh_own_keys(op);
            assert!(!okeys.is_empty(), "Object.prototype lists no keys");
            for &(k, h) in &fkeys {
                assert_eq!(super::test_desc_holder(fp, k, h), f_node, "own key {k:#x}");
            }
            for &(k, h) in okeys.iter().filter(|x| !fkeys.iter().any(|f| f.0 == x.0)) {
                assert_eq!(
                    super::test_desc_holder(fp, k, h),
                    o_node,
                    "inherited key {k:#x}"
                );
            }
        }
    }
}
