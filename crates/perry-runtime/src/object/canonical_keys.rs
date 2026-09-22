//! Canonical keys arrays: the ADDRESS is the content identity (#10868 step
//! 2.5 stage 1b).
//!
//! `facts_key` folds SIX identity facts and one of them is the keys array's
//! ADDRESS. Two objects whose ordered key lists are byte-identical but whose
//! arrays were allocated separately therefore mint two ShapeIds for one
//! layout. On a real `ts.transpileModule` that is 32,246 of 42,097 mints
//! (`key_count` 19,923 plus `fresh_keys_known_list` 12,323) against a process
//! that ABORTS when the id space runs out.
//!
//! The fix is not to change `facts_key`. It is to make the address tell the
//! truth: obtain every keys array from here and exactly one array exists per
//! distinct ordered key list, so folding the pointer IS folding the content,
//! and the probe path stays byte-for-byte what it was.
//!
//! ## The structure is a TRIE, so `extend_slot` is O(1) and no content is hashed
//!
//! L8.3.15 named the one hard problem: `facts_key` is O(1) because it folds a
//! pointer, and content is O(N). A content-hash intern table moves that O(N)
//! off the probe path onto array creation — better, but still O(N) per grow
//! and O(N^2) to build an N-key object one key at a time.
//!
//! Canonical arrays form a TREE. Every one of them is (canonical parent, one
//! appended slot), rooted at the empty list. So the table is an EDGE map, not
//! a content map; [`extend_slot`] is one hash probe on `(parent node, slot hash)`
//! plus an exact check of the single appended slot; and no content is ever
//! walked on the grow path. [`canonicalize`] — the general constructor, for a
//! producer that hands over a whole list — is a fold of `extend_slot` over the
//! slots: the same one path N times, not a second path.
//!
//! ## Node ids, not addresses, so the collector touches one `Vec`
//!
//! Edges are keyed by NODE ID, which the collector cannot move. Only
//! `Node::addr` and the address index are address-typed, so a minor visits a
//! contiguous `Vec<Node>` and patches a handful of index entries, instead of
//! rekeying an address-keyed edge map the way `move_shape_family` must rekey
//! `families`.
//!
//! ## Weak, exactly like the transition cache — and that is what bounds it
//!
//! #6759 phase 3 made the transition cache's `next_keys` WEAK (rewritten on
//! move, dropped on death) after strong rooting pinned up to 16,384 keys
//! arrays and, through them, their descriptors: 786k descriptors on a
//! workload holding under 400 live objects. This table is weak for the same
//! reason, through the same two mechanisms — [`scan_canonical_keys_roots_mut`]
//! and [`prune_dead_canonical_keys`].
//!
//! That answers L8.3.15c's retention worry, which assumed the intern table
//! would hold its arrays: **it holds none**. A node whose array died is
//! dropped, retention is proportional to LIVE layouts, and step 2.5 therefore
//! introduces no latch trigger of its own. (`ShapeObjectKind::Dictionary`
//! ships in the same PR regardless, per L8.3.15f, so whichever PR later wires
//! a trigger already has the kind beneath it.)
//!
//! Dropping a node whose array died can ORPHAN its children: they stay
//! adoptable and extendable, but a later walk from the root rebuilds the chain
//! and mints one duplicate layout. That is a mint, never a wrong answer, and
//! `fresh_keys_known_list` in the mint census is precisely its witness — which
//! is why the census, not a perf gate, is the acceptance instrument here.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::array::ArrayHeader;
use crate::JSValue;
use crate::StringHeader;

/// A keys array this table owns.
///
/// The enforcement half of the funnel, and the reason it is a type rather
/// than a comment: only this module can mint one, every producer of a keys
/// array must accept one, and it exposes no writable element pointer. So "a
/// producer forgot to canonicalize" is a compile error rather than a silent
/// duplicate layout, and "a mutator edited a shared array in place" is a
/// compile error rather than one object's key edit appearing in every object
/// of that layout.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct CanonicalKeys(*mut ArrayHeader);

impl CanonicalKeys {
    /// The empty ordered key list. A keyless shape's `keys` fact is 0, so the
    /// root of the trie owns no array and costs nothing.
    pub(crate) const EMPTY: CanonicalKeys = CanonicalKeys(std::ptr::null_mut());

    #[inline]
    pub(crate) fn as_ptr(self) -> *mut ArrayHeader {
        self.0
    }

    #[inline]
    pub(crate) fn as_const_ptr(self) -> *const ArrayHeader {
        self.0 as *const ArrayHeader
    }

    #[inline]
    pub(crate) fn addr(self) -> usize {
        self.0 as usize
    }

    #[inline]
    pub(crate) fn is_empty(self) -> bool {
        self.0.is_null()
    }

    /// The live key count, which equals the array's `length` by construction:
    /// a canonical array is exactly as long as the list it names, because a
    /// prefix is its own node.
    ///
    /// # Safety
    /// The table prunes a dead address before anything can read it, so a
    /// handle in hand names a live array.
    #[inline]
    pub(crate) fn len(self) -> u32 {
        if self.0.is_null() {
            0
        } else {
            unsafe { (*self.0).length }
        }
    }
}

/// A live object pointer that a call which may collect has to hand back.
///
/// **Not `Copy` and not `Clone`**, which is the whole mechanism: a function
/// that can allocate takes this BY VALUE and returns the post-collection one,
/// so a caller that keeps using its old binding is a *move-after-use* error
/// at compile time rather than a receiver written at a stale address at run
/// time.
///
/// It exists because I wrote `CanonicalKeys` to make "a producer forgot to
/// canonicalize" impossible and then, four hours later, introduced the twin
/// defect myself: `canonicalize` made `shape_cache_insert` allocate, and
/// `js_object_alloc_class_with_keys` held the object under construction as a
/// raw pointer across it, so a collection there moved the receiver and the
/// keys edge landed in the freed address. Lane 16b predicted exactly this
/// class at `delete_rest.rs:412` and noted that step 2.5 is what makes the
/// path allocate — the prediction and its confirmation are four hours apart.
///
/// Nobody can enumerate the members of this class by inspection; that is the
/// argument for a type rather than three hand-rooted call sites.
///
/// `none()` is for a caller that has no unrooted object to carry — either
/// none exists yet, or it is already held in a `RuntimeHandleScope`, which is
/// the older discipline and is what `js_object_alloc_with_shape` does.
pub(crate) struct LiveObject(*mut crate::object::ObjectHeader);

impl LiveObject {
    #[inline]
    pub(crate) fn new(obj: *mut crate::object::ObjectHeader) -> Self {
        LiveObject(obj)
    }

    /// No unrooted object crosses this call.
    #[inline]
    pub(crate) fn none() -> Self {
        LiveObject(std::ptr::null_mut())
    }

    /// The raw pointer, at a leaf that does not allocate.
    #[inline]
    pub(crate) fn as_ptr(&self) -> *mut crate::object::ObjectHeader {
        self.0
    }

    /// Run `f` with this object rooted, and return the token carrying its
    /// post-collection address. Every allocating callee that accepts a
    /// `LiveObject` funnels through here.
    pub(crate) fn across<R>(self, f: impl FnOnce() -> R) -> (Self, R) {
        if self.0.is_null() {
            let out = f();
            return (self, out);
        }
        let scope = crate::gc::RuntimeHandleScope::new();
        let handle = scope.root_raw_mut_ptr(self.0);
        let (out, obj) = handle.across_mut(f);
        (LiveObject(obj), out)
    }
}

/// Proof that a key list describes a **shared layout**, not one receiver's
/// private list — the receiver side of `CanonicalKeys`.
///
/// `canonicalize` and `extend_slot` REQUIRE one, so interning cannot run without
/// it. That is deliberately a type and not an ordering: stage 1a added
/// `ShapeObjectKind::Dictionary` precisely so a latched receiver would
/// decline by construction, and stage 1b then added a path that never asked
/// — three canonicalization calls sat ABOVE every `is_dictionary` guard, so
/// a latched receiver had its private list interned and republished as a
/// shared array and lost 407 of 8,192 keys. Hoisting those three guards
/// would have fixed those three sites and left the fourth, next month, to
/// someone as careful as I was.
///
/// Nobody holds the membership of this class in their head, including the
/// author of the rule. So the rule is a constructor.
pub(crate) struct SharedLayout {
    /// True only for the shape cache's own entry. DISCRIMINATOR (#9754):
    /// `scan_shape_cache_roots_mut` states that the shape cache's keys arrays
    /// live in the LONGLIVED arena. If that assumption is confined to the
    /// cache, longlived HERE and ordinary on the grow path is enough.
    shape_cache: bool,
}

impl SharedLayout {
    /// The kind check, and the only way a receiver yields the proof.
    /// `None` for a dictionary receiver: its keys are its own, it appends
    /// them in place, and interning them would publish one object's private
    /// list as every object's layout.
    ///
    /// # Safety
    /// `obj` is a live object header, or null.
    #[inline]
    pub(crate) unsafe fn of_receiver(obj: *mut crate::object::ObjectHeader) -> Option<Self> {
        if obj.is_null() || crate::object::dictionary::is_dictionary(obj) {
            return None;
        }
        Some(SharedLayout { shape_cache: false })
    }

    /// The shape cache's entries are shared layouts by construction: they are
    /// keyed by a STATIC shape id and handed to every receiver of that shape,
    /// so there is no receiver whose kind could make them private. The one
    /// place the proof is not a kind check, named so it is auditable rather
    /// than implicit.
    #[inline]
    pub(crate) fn shape_cache_entry() -> Self {
        SharedLayout { shape_cache: true }
    }
}

const NO_NODE: u32 = u32::MAX;
/// The empty list. Never freed, owns no array.
const ROOT_NODE: u32 = 0;

struct Node {
    /// The canonical array. 0 for the root and for a free slot.
    addr: usize,
    parent: u32,
    edge_hash: u64,
    /// Next node in this node's `(parent, edge_hash)` bucket, or the free-list
    /// link when the slot is free.
    next: u32,
    len: u32,
    /// Every slot of this list is a heap string POINTER.
    ///
    /// Carried because it decides which allocator the child uses, and
    /// getting that wrong is expensive in a way no test would show: an array
    /// built with the raw-f64 allocator and repaired by
    /// `rebuild_array_layout_from_slots` gets a per-object side mask, and ONE
    /// live entry in that table arms `PERRY_PER_OBJECT_LAYOUTS_ANY` for the
    /// whole program — the address-filter probe on every later allocation,
    /// measured at 3% (`alloc.rs`'s #7510 note). A child's answer is its
    /// parent's AND the appended slot's, so it costs one bit and no walk.
    all_ptr: bool,
}

/// The canonical trie for one agent.
pub(crate) struct CanonicalTable {
    nodes: Vec<Node>,
    free: u32,
    /// Canonical array address -> node id.
    by_addr: HashMap<usize, u32>,
    /// `(parent node, appended-slot hash)` -> first candidate node.
    edges: HashMap<(u32, u64), u32>,
    minted: u64,
    reaped: u64,
}

impl CanonicalTable {
    fn new() -> Self {
        Self {
            nodes: vec![Node {
                addr: 0,
                parent: NO_NODE,
                edge_hash: 0,
                next: NO_NODE,
                len: 0,
                // The empty list is vacuously all-pointer, which is what makes
                // a one-key list's answer just "is this key a heap string".
                all_ptr: true,
            }],
            free: NO_NODE,
            by_addr: HashMap::new(),
            edges: HashMap::new(),
            minted: 0,
            reaped: 0,
        }
    }

    fn alloc_node(
        &mut self,
        addr: usize,
        parent: u32,
        edge_hash: u64,
        len: u32,
        all_ptr: bool,
    ) -> u32 {
        let fresh = Node {
            addr,
            parent,
            edge_hash,
            next: NO_NODE,
            len,
            all_ptr,
        };
        let id = if self.free != NO_NODE {
            let id = self.free;
            self.free = self.nodes[id as usize].next;
            self.nodes[id as usize] = fresh;
            id
        } else {
            self.nodes.push(fresh);
            (self.nodes.len() - 1) as u32
        };
        self.minted += 1;
        CANON_MINTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        CANON_LIVE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        CANON_WORDS.fetch_add(u64::from(len), std::sync::atomic::Ordering::Relaxed);
        self.by_addr.insert(addr, id);
        if parent != NO_NODE {
            let head = self.edges.entry((parent, edge_hash)).or_insert(NO_NODE);
            self.nodes[id as usize].next = *head;
            *head = id;
        }
        id
    }

    /// Unlink `id`, orphan its children and return its slot to the free list.
    /// The children keep their arrays and stay adoptable; they merely stop
    /// being reachable from the root. See the module note on orphans.
    fn free_node(&mut self, id: u32) {
        if id == ROOT_NODE || id as usize >= self.nodes.len() {
            return;
        }
        let (parent, edge_hash, addr, next_sib, freed_len) = {
            let n = &self.nodes[id as usize];
            (n.parent, n.edge_hash, n.addr, n.next, n.len)
        };
        if addr == 0 {
            return;
        }
        if parent != NO_NODE {
            if let Some(&head) = self.edges.get(&(parent, edge_hash)) {
                if head == id {
                    if next_sib == NO_NODE {
                        self.edges.remove(&(parent, edge_hash));
                    } else {
                        self.edges.insert((parent, edge_hash), next_sib);
                    }
                } else {
                    let mut cur = head;
                    while cur != NO_NODE {
                        let n = self.nodes[cur as usize].next;
                        if n == id {
                            self.nodes[cur as usize].next = next_sib;
                            break;
                        }
                        cur = n;
                    }
                }
            }
        }
        self.by_addr.remove(&addr);
        // A freed slot may be reused by an unrelated node, so nothing may keep
        // pointing at this id: detach every child bucket first.
        let mut orphans: Vec<u32> = Vec::new();
        for (key, head) in self.edges.iter() {
            if key.0 == id {
                let mut cur = *head;
                while cur != NO_NODE {
                    orphans.push(cur);
                    cur = self.nodes[cur as usize].next;
                }
            }
        }
        self.edges.retain(|key, _| key.0 != id);
        for child in orphans {
            self.nodes[child as usize].parent = NO_NODE;
            self.nodes[child as usize].next = NO_NODE;
        }
        self.nodes[id as usize] = Node {
            addr: 0,
            parent: NO_NODE,
            edge_hash: 0,
            next: self.free,
            len: 0,
            all_ptr: true,
        };
        self.free = id;
        self.reaped += 1;
        CANON_REAPED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        CANON_LIVE.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        CANON_WORDS.fetch_sub(u64::from(freed_len), std::sync::atomic::Ordering::Relaxed);
    }
}

crate::perry_thread_local! {
    static CANONICAL_KEYS: RefCell<CanonicalTable> = RefCell::new(CanonicalTable::new());
}

/// Diagnostic totals across every agent thread.
///
/// PROCESS-global on purpose, where the table is thread-local: user code runs
/// on a dedicated thread and the mint census reports from another, so reading
/// the reporting thread's (empty) table printed `live 0` for a program that
/// had just built a full trie. A census that reports on the wrong thread is
/// the same defect as one that measures nothing. Nothing branches on these,
/// and no test asserts them, so they are plain statics rather than
/// `per_test_global!` sinks.
static CANON_LIVE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static CANON_MINTED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static CANON_REAPED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Element words held by live canonical arrays — the side-table bytes this
/// stage is measured on, times eight.
static CANON_WORDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Every access goes through `try_with`, never `with`.
///
/// A thread being torn down has already dropped its thread-locals, and `with`
/// PANICS there ("cannot access a Thread Local Storage value during or after
/// destruction"). Both of this module's callers can run on a dying thread —
/// the collector's root scan and dead-key prune, and the mint census, which
/// reports at process exit — so the panic was reachable, and was OBSERVED on
/// the first measured run before this was written. A table that is gone holds
/// no canonical arrays, so `None` is not a lost answer; it is the right one.
fn try_with_table<R>(f: impl FnOnce(&mut CanonicalTable) -> R) -> Option<R> {
    CANONICAL_KEYS.try_with(|t| f(&mut t.borrow_mut())).ok()
}

/// [`try_with_table`] for a caller that must produce a value. `default` is
/// what a torn-down table means for that caller, stated at the call site.
fn with_table_or<R>(default: R, f: impl FnOnce(&mut CanonicalTable) -> R) -> R {
    try_with_table(f).unwrap_or(default)
}

/// The one slot an `extend_slot` appends, in whichever form the caller has it.
///
/// Two constructors, one path: both produce the same edge hash for the same
/// key bytes and both validate by BYTES, so a grow (which holds an interned
/// key string) and a rebuild (which holds a stored slot) reach the same node.
#[derive(Clone, Copy)]
pub(crate) enum Appended {
    /// An incoming interned key string, as the grow path holds it.
    Key(*const StringHeader),
    /// A slot read out of a keys array — a key, a tombstone or a symbol.
    Slot(JSValue),
}

impl Appended {
    /// # Safety
    /// The operand is live.
    unsafe fn edge_hash(self) -> u64 {
        match self {
            Appended::Key(key) => {
                if key.is_null() {
                    return 0x4E55_4C4C_4B45_5900;
                }
                let data = (key as *const u8).add(std::mem::size_of::<StringHeader>());
                crate::object::keys_lookup::key_bytes_hash(data, (*key).byte_len as usize)
            }
            Appended::Slot(v) => {
                let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
                match crate::string::js_string_key_bytes(v, &mut sso) {
                    Some(bytes) => {
                        crate::object::keys_lookup::key_bytes_hash(bytes.as_ptr(), bytes.len())
                    }
                    // A hole or a symbol is part of the ordered list and must
                    // participate, or `{a, <hole>}` and `{a, b}` share an edge
                    // — the ORDER/tombstone-POSITION merge that L8.3.15's
                    // must-fail control exists to catch.
                    None => v.bits() ^ 0x9E37_79B9_7F4A_7C15,
                }
            }
        }
    }

    /// Does `stored` — the last slot of a candidate child — name this key?
    /// The hash is never trusted on its own: a wrong array would be a wrong
    /// key list for every object of that layout.
    ///
    /// # Safety
    /// Both operands are live.
    unsafe fn matches(self, stored: JSValue) -> bool {
        match self {
            // A null key never matches by BYTES, so without this it would
            // miss its own node on every probe and mint an array per call —
            // an unbounded source of exactly the duplicates this module
            // exists to remove. Callers guard against null earlier; this
            // makes the guard's absence a shared node rather than a leak.
            Appended::Key(key) if key.is_null() => {
                stored.bits() == crate::value::js_nanbox_string(0).to_bits()
            }
            Appended::Key(key) => crate::string::js_string_key_matches(stored, key),
            Appended::Slot(v) => {
                if stored.bits() == v.bits() {
                    return true;
                }
                let mut sa = [0u8; crate::value::SHORT_STRING_MAX_LEN];
                let mut sb = [0u8; crate::value::SHORT_STRING_MAX_LEN];
                let a = crate::string::js_string_key_bytes(stored, &mut sa);
                let b = crate::string::js_string_key_bytes(v, &mut sb);
                match (a, b) {
                    (Some(x), Some(y)) => x == y,
                    // A non-string pair that is not bit-equal is a different
                    // list; the bit compare above already accepted an
                    // identical hole.
                    _ => false,
                }
            }
        }
    }

    /// The raw element word to write into the appended slot.
    ///
    /// # Safety
    /// The operand is live.
    unsafe fn element_word(self) -> f64 {
        match self {
            Appended::Key(key) => crate::value::js_nanbox_string(key as i64),
            Appended::Slot(v) => f64::from_bits(v.bits()),
        }
    }

    /// Is the appended slot a heap string POINTER? An SSO short string and a
    /// tombstone are not, and either one costs the child its all-pointer
    /// layout — see `Node::all_ptr`.
    #[inline]
    fn is_pointer(self) -> bool {
        match self {
            Appended::Key(key) => !key.is_null(),
            Appended::Slot(v) => v.is_string(),
        }
    }
}

/// The node id of a canonical handle, or `None` when a death prune dropped it.
fn node_of(t: &CanonicalTable, keys: CanonicalKeys) -> Option<u32> {
    if keys.is_empty() {
        return Some(ROOT_NODE);
    }
    t.by_addr.get(&keys.addr()).copied()
}

/// Probe the trie for `parent + appended`, validating the appended slot
/// exactly.
///
/// # Safety
/// `parent` names a live canonical array (or is empty) and `appended` is live.
unsafe fn probe(
    parent: CanonicalKeys,
    parent_len: u32,
    appended: Appended,
    h: u64,
) -> Option<CanonicalKeys> {
    with_table_or(None, |t| {
        let pnode = node_of(t, parent)?;
        let mut cur = *t.edges.get(&(pnode, h))?;
        while cur != NO_NODE {
            let (addr, len, next) = {
                let node = &t.nodes[cur as usize];
                (node.addr, node.len, node.next)
            };
            if addr != 0 && len == parent_len + 1 {
                let arr = addr as *const ArrayHeader;
                // Keep the cache entry's exact-length check before resolving
                // slots: a forwarded stub is not a canonical array to return.
                if (*arr).length == parent_len + 1 {
                    let (slots, slot_len) = crate::object::keys_array_dense_slots(arr);
                    if parent_len as usize >= slot_len {
                        cur = next;
                        continue;
                    }
                    #[cfg(test)]
                    canonical_keys_tests::note_slot_read();
                    // Compare the internal key slot, not JavaScript Get:
                    // Get translates a tombstone into undefined (or a
                    // prototype value), which would miss the same trie edge.
                    let stored = JSValue::from_bits((*slots.add(parent_len as usize)).to_bits());
                    if appended.matches(stored) {
                        return Some(CanonicalKeys(addr as *mut ArrayHeader));
                    }
                }
            }
            cur = next;
        }
        None
    })
}

/// Stamp the invariant every canonical array carries: it is shared from
/// birth, so copy-on-write is the only append path rather than the fallback
/// one (L8.3.15c).
///
/// # Safety
/// `arr` is a live, tracked array.
unsafe fn stamp_shared(arr: *mut ArrayHeader) {
    let gc_header = crate::value::addr_class::try_read_tracked_gc_header(arr as usize)
        .expect("a canonical array must be a tracked GC allocation");
    (*gc_header.as_ptr()).gc_flags |= crate::gc::GC_FLAG_SHAPE_SHARED;
}

/// The canonical array for `parent`'s ordered key list with one slot
/// appended. O(1): one hash probe and one exact slot check; allocates only on
/// a genuinely new layout.
///
/// # Safety
/// `parent` names a live canonical array (or is empty), `appended` is live,
/// and the caller has rooted everything it holds across the allocation this
/// may perform.
pub(crate) unsafe fn extend_slot(
    _proof: &SharedLayout,
    parent: CanonicalKeys,
    appended: Appended,
) -> CanonicalKeys {
    let h = appended.edge_hash();
    let parent_len = parent.len();
    if let Some(hit) = probe(parent, parent_len, appended, h) {
        return hit;
    }

    // Whether the child's slots are all heap string pointers is the parent's
    // answer AND this slot's, so it is read before the allocation and never
    // re-derived by walking the list.
    // A torn-down table cannot promise an all-pointer layout, so `false` is
    // the safe default: the mask path is correct for any content.
    let all_ptr = with_table_or(false, |t| {
        node_of(t, parent)
            .map(|id| t.nodes[id as usize].all_ptr)
            .unwrap_or(false)
    }) && appended.is_pointer();

    // Nothing may be held across the allocation: no table borrow (a collection
    // re-enters this table through its scanner and its prune) and both
    // operands rooted, because a collection here moves them.
    let scope = crate::gc::RuntimeHandleScope::new();
    let parent_handle = scope.root_raw_mut_ptr(parent.0);
    let appended_handle = match appended {
        Appended::Key(key) => scope.root_string_ptr(key),
        Appended::Slot(v) => scope.root_nanbox_u64(v.bits()),
    };
    // The all-pointer allocator, when the claim holds: it declares the layout
    // in the header, where `rebuild_array_layout_from_slots` would install a
    // per-object side mask — and one live mask entry arms the address filter
    // for the whole program (`Node::all_ptr`).
    // LONGLIVED, and still load-bearing — but NOT for the reason the first
    // version of this comment gave.
    //
    // It was introduced because `CLASS_KEYS_BY_ID` held the shape cache's
    // keys array as an unscanned raw `usize`. That table is scanned and
    // pruned now (`alloc.rs`), and scanning it DID buy what was predicted:
    // with ordinary allocation, all seven of the GC tests that assert a keys
    // array moves, is rewritten, and is reclaimed pass UNCHANGED, and the
    // suite's failures fall 12 -> 6.
    //
    // But `descriptor_trap_collection_preserves_for_in_target_and_keys`
    // returns 1 key of 14 again the moment allocation goes back to the
    // nursery. So `CLASS_KEYS_BY_ID` was NOT the holder — or not the only
    // one — and the earlier root cause was a site that had been localised,
    // not explained. Same error as L8.3.21, caught the same way: by the fix
    // failing to fix it.
    //
    // A wrong answer must not ship, so the allocator stays until the real
    // holder is found. The cost is the retention regression L8.3.15c was
    // withdrawn for, which seven tests name precisely and which is bounded
    // (135 KB on tsc; majors still reclaim).
    let allocate = || {
        if _proof.shape_cache {
            let a = crate::array::js_array_alloc_with_length_longlived(parent_len + 1);
            (*a).length = 0;
            if all_ptr {
                crate::gc::layout_init_all_pointer_slots(a as *mut u8);
            }
            a
        } else if all_ptr {
            crate::array::js_array_alloc_pointer_elements(parent_len + 1)
        } else {
            crate::array::js_array_alloc(parent_len + 1)
        }
    };
    // Reload both operands only after the child allocation can no longer
    // move them. No GC allocation occurs while the fresh array is filled.
    let ((fresh, parent), appended) = match appended {
        Appended::Key(_) => {
            let (result, key) = appended_handle.across_const::<StringHeader, _>(|| {
                parent_handle.across_mut::<ArrayHeader, _>(allocate)
            });
            (result, Appended::Key(key))
        }
        Appended::Slot(_) => {
            let (result, slot) = appended_handle
                .across_nanbox(|| parent_handle.across_mut::<ArrayHeader, _>(allocate));
            (result, Appended::Slot(JSValue::from_bits(slot.to_bits())))
        }
    };
    let parent = CanonicalKeys(parent);

    // A collection during the allocation may have published this exact node
    // through another path, or pruned the parent. Re-probe before writing.
    if let Some(hit) = probe(parent, parent_len, appended, h) {
        return hit;
    }

    // #10939/#10948: all three former clone-before-append sites funnel here.
    // Resolve grow-forward pointers and front reserves on the source, and use
    // the destination's element base. Publishing a shape count beyond the
    // initialized prefix would make the collector trace uninitialized words.
    let (src, src_len) = crate::object::keys_array_dense_slots(parent.as_const_ptr());
    let copied = (parent_len as usize).min(src_len);
    debug_assert_eq!(
        copied, parent_len as usize,
        "the shape's key count outruns its keys array"
    );
    let parent_len = copied as u32;
    let dst = crate::array::array_elements_ptr(fresh as *const ArrayHeader) as *mut f64;
    if parent_len > 0 {
        for i in 0..copied {
            // GC_STORE_AUDIT(INIT): `fresh` is unpublished, and its length —
            // which bounds every collector view of it — is set only after the
            // last slot is written.
            *dst.add(i) = *src.add(i);
        }
    }
    *dst.add(parent_len as usize) = appended.element_word();
    (*fresh).length = parent_len + 1;
    if !all_ptr {
        // Only the mixed list needs the slot walk; the all-pointer allocator
        // already declared its layout in the header, and publishing `length`
        // after the last write is the precondition it documents.
        crate::object::gc_slots::rebuild_array_layout_from_slots(fresh);
    }
    stamp_shared(fresh);

    try_with_table(|t| {
        // The parent may have been pruned while we allocated. Keep the array —
        // it is a correct list — as an orphan root: the caller still gets the
        // right content and only the edge is lost.
        let pnode = node_of(t, parent).unwrap_or(NO_NODE);
        t.alloc_node(fresh as usize, pnode, h, parent_len + 1, all_ptr);
    });
    CanonicalKeys(fresh)
}

/// [`extend_slot`] with an incoming interned key string — the grow path's form.
///
/// # Safety
/// As [`extend_slot`].
#[inline]
pub(crate) unsafe fn extend_key(
    proof: &SharedLayout,
    parent: CanonicalKeys,
    key: *const StringHeader,
) -> CanonicalKeys {
    extend_slot(proof, parent, Appended::Key(key))
}

/// The canonical array for the ordered key list held in `keys[0..len]`.
///
/// The funnel's total form: whatever a producer hands in — a private clone, a
/// cache entry, a freshly built list — what comes back is THE array for that
/// content. When `keys` is already canonical at that length it is returned
/// unchanged, which is every call after the first.
///
/// # Safety
/// `keys` is a live keys array (or null) with at least `len` initialized
/// slots, and the caller has rooted what it holds: this allocates.
pub(crate) unsafe fn canonicalize(
    proof: &SharedLayout,
    keys: *const ArrayHeader,
    len: u32,
) -> CanonicalKeys {
    if keys.is_null() || len == 0 {
        return CanonicalKeys::EMPTY;
    }
    let already = with_table_or(false, |t| {
        t.by_addr
            .get(&(keys as usize))
            .copied()
            .map(|id| t.nodes[id as usize].len == len)
            .unwrap_or(false)
    });
    if already {
        return CanonicalKeys(keys as *mut ArrayHeader);
    }

    // Fold `extend_slot` over the slots. One path, N times — the source is re-read
    // after every step, because `extend_slot` allocates and the collector moves it.
    let scope = crate::gc::RuntimeHandleScope::new();
    let src = scope.root_raw_mut_ptr(keys as *mut ArrayHeader);
    let mut out = CanonicalKeys::EMPTY;
    for i in 0..len {
        let slot = src.with_const_ptr(|arr| {
            // Re-resolve grow-forward pointers and front reserves after each
            // allocating extension; borrow slots only for this read.
            let (slots, src_len) = crate::object::keys_array_dense_slots(arr);
            let copied = (len as usize).min(src_len);
            debug_assert_eq!(
                copied, len as usize,
                "the shape's key count outruns its keys array"
            );
            ((i as usize) < copied).then(|| JSValue::from_bits((*slots.add(i as usize)).to_bits()))
        });
        let Some(slot) = slot else { break };
        out = extend_slot(proof, out, Appended::Slot(slot));
    }
    out
}

/// GC root scanner. The arrays are WEAK: rewritten on move, never marked —
/// see the module note on #6759 phase 3.
pub fn scan_canonical_keys_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let young = visitor.young_scope();
    let mut moved: Vec<(usize, usize, u32)> = Vec::new();
    let _ = CANONICAL_KEYS.try_with(|t| {
        let mut t = t.borrow_mut();
        for id in 1..t.nodes.len() {
            let addr = t.nodes[id].addr;
            if addr == 0 {
                continue;
            }
            // A minor can only move a young address, and the node table is one
            // contiguous `Vec` — so the address check is cheaper here than a
            // fourth young log would be.
            if young && !crate::gc::young_log::addr_is_minor_relevant(addr) {
                continue;
            }
            let mut next = addr;
            if visitor.visit_metadata_usize_slot(&mut next) && next != addr {
                t.nodes[id].addr = next;
                moved.push((addr, next, id as u32));
            }
        }
        for (old, new, id) in moved.drain(..) {
            t.by_addr.remove(&old);
            t.by_addr.insert(new, id);
        }
    });
}

/// Post-trace death prune. A node whose array did not survive is dropped; its
/// children are orphaned rather than followed, which costs at most one
/// duplicate layout and never a wrong list.
#[cold]
pub(crate) fn prune_dead_canonical_keys(is_dead_owner: &dyn Fn(usize) -> bool) {
    // Snapshot first, then ask. `is_dead_owner` is a collector predicate this
    // module does not own, and calling it under the table borrow would make a
    // future re-entrant reader a panic rather than a slow path.
    let live: Vec<(u32, usize)> = CANONICAL_KEYS
        .try_with(|t| {
            let t = t.borrow();
            (1..t.nodes.len())
                .filter(|&id| t.nodes[id].addr != 0)
                .map(|id| (id as u32, t.nodes[id].addr))
                .collect()
        })
        .unwrap_or_default();
    let dead: Vec<u32> = live
        .into_iter()
        .filter(|&(_, addr)| is_dead_owner(addr) || canonical_address_is_recycled(addr))
        .map(|(id, _)| id)
        .collect();
    if dead.is_empty() {
        return;
    }
    try_with_table(|t| {
        for id in dead {
            t.free_node(id);
        }
    });
}

/// An address is only a keys array while the cell at it still IS an array:
/// the arena recycles addresses, and a recycled tenant answers "alive" to
/// `is_dead_owner`. Twin of `shape_keys_address_is_recycled`.
fn canonical_address_is_recycled(addr: usize) -> bool {
    // SAFETY: a read-only tracked-header probe, which tolerates an address
    // that is no longer a tracked cell.
    unsafe {
        match crate::value::addr_class::try_read_tracked_gc_header(addr) {
            Some(gc) => {
                let ty = (*gc.as_ptr()).obj_type;
                ty != crate::gc::GC_TYPE_ARRAY && ty != crate::gc::GC_TYPE_LAZY_ARRAY
            }
            None => true,
        }
    }
}

/// `(live nodes, nodes ever minted, nodes reaped)`. The census reads this;
/// nothing branches on it.
pub(crate) fn canonical_stats() -> (usize, u64, u64) {
    (
        CANON_LIVE.load(std::sync::atomic::Ordering::Relaxed) as usize,
        CANON_MINTED.load(std::sync::atomic::Ordering::Relaxed),
        CANON_REAPED.load(std::sync::atomic::Ordering::Relaxed),
    )
}

/// Total element words held by live canonical arrays — the side-table bytes
/// this stage is measured on.
pub(crate) fn canonical_element_words() -> u64 {
    CANON_WORDS.load(std::sync::atomic::Ordering::Relaxed)
}

#[cfg(test)]
pub(crate) fn reset_for_test() {
    let _ = CANONICAL_KEYS.try_with(|t| *t.borrow_mut() = CanonicalTable::new());
}

#[cfg(test)]
#[path = "canonical_keys_tests.rs"]
mod canonical_keys_tests;
