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
//! walked on a grow hit. [`canonicalize`] walks the same edges for a whole
//! list, but materializes only the requested leaf. Unpublished prefixes use
//! a weak descendant witness for exact slot validation.
//!
//! ## One BACKING per growth chain (V8's descriptor-array sharing)
//!
//! The lists on a linear chain — `[a]`, `[a,b]`, `[a,b,c]` — are prefixes of
//! each other, so they share ONE backing array that holds the tip's keys. A
//! node is `(backing, count)`: `[a,b]` is "the first two entries of that
//! backing". Extending the tip by one key appends in place ([`extend_slot`]):
//! amortized growth, no copy, no new array. A FORK — a different key appended
//! to a node the backing has already grown past — copies that node's prefix
//! into a new backing and continues there, as does a tip whose backing is
//! full.
//!
//! What makes this sound:
//! * the SHAPE owns the count. A descriptor is `(keys, logical_key_count)`,
//!   and every consumer reads an object's keys through
//!   [`crate::object::ObjectKeys`], which has no way to read past its count.
//!   A backing's header `length` is the tip's count and nobody else's;
//! * a backing's prefix is immutable. The only in-place write to a backing
//!   is this module's tip append, past every published count. Every other
//!   writer refuses a `GC_FLAG_SHAPE_SHARED` array and copies first, and every
//!   backing carries that flag from birth. A confirmed heap-string key may
//!   substitute its equal immutable image header, just as relocation changes
//!   its address without changing the prefix's string value;
//! * the collector sees one ordinary array: its header length covers every
//!   written slot, it is traced as a whole and never moves, and nodes and
//!   descriptors hold `(backing, count)` — never an interior pointer. A key
//!   string that moves is rewritten in the backing once, which is the right
//!   answer for every list sharing it: they name the same key.
//!
//! ## Stable node ids and stationary backing addresses
//!
//! Edges are keyed by NODE ID, which the collector cannot move. The backing
//! addresses never move, so nodes need no relocation scanner. Full traces
//! prune unreachable backings.
//!
//! ## The node is the list's identity, carried by its holders (CKI S1)
//!
//! There is no `(address, count)` -> node index. A [`CanonicalKeys`] handle,
//! an [`crate::object::ObjectKeys`] view and the shape record each carry the
//! node of their list, and every consumer validates the node it was handed
//! with one read (`validated_node`). An index keyed by image and heap
//! addresses made every probe's cost a function of the link layout.
//!
//! ## What a list owns in the shape table lives on its node (CKI S2)
//!
//! A node also holds its list's shape state ([`crate::object::shapes::ListShapes`]):
//! the FAMILY (every descriptor minted on this list) and, on the node that owns
//! a backing (`NodeShapes::backing_owner`), the backing's slot index. Both used to
//! be maps keyed by the keys array's address, an image address for every image
//! list. Only owned (dictionary) arrays keep address-keyed entries, and those
//! are heap addresses, fixed by the allocation sequence rather than the link.
//!
//! ## Weak, exactly like the transition cache — and that is what bounds it
//!
//! #6759 phase 3 made the transition cache's `next_keys` WEAK (rewritten on
//! move, dropped on death) after strong rooting pinned up to 16,384 keys
//! arrays and, through them, their descriptors: 786k descriptors on a
//! workload holding under 400 live objects. This table is weak for the same
//! reason, through [`prune_dead_canonical_keys`] after full traces. I3 backings are
//! born old and movement-pinned: minors cannot move or reclaim them.
//!
//! That answers L8.3.15c's retention worry, which assumed the intern table
//! would hold its arrays: **it holds none**. A node whose array died is
//! dropped, and retention is proportional to LIVE layouts. What the table
//! does supply is the dictionary latch's signal: a node's unique RUN, the keys
//! its lineage grew by one receiver's append at a time since another arrival
//! reached it ([`take_unique_run`]).
//!
//! A node whose array died is not dropped while a live list extends it: it
//! stays as that list's unpublished prefix, validated by the live array
//! ([`prune_dead_canonical_keys`]). Dropping it would orphan the live list,
//! and the next walk from the root would publish a second array for the same
//! keys: a second shape for one layout, and a static ShapeId (which names the
//! one canonical list of its keys) refused by the mint of its own facts.

use crate::fast_hash::{new_fast_key_hash_map, FastKeyHashMap};
use std::cell::RefCell;

use crate::array::ArrayHeader;
use crate::JSValue;
use crate::StringHeader;

pub(crate) mod static_seed;
pub(crate) use static_seed::from_static_layout;
pub(crate) use static_seed::link_image_list;

/// Retain the image spelling of a confirmed equal key in its existing owner.
/// This changes only a string's representation, never its value, position,
/// attributes or layout. Heap-to-heap substitution preserves pointer masks.
/// No registry or additional site state is needed; later reads compare words.
///
/// # Safety
/// `keys[index]` and `key` are live, equal strings in a canonical backing.
pub(crate) unsafe fn adopt_image_key(
    keys: *mut ArrayHeader,
    index: usize,
    key: *const StringHeader,
) {
    if !crate::string::is_image_string(key) {
        return;
    }
    let (slots, len) = crate::object::keys_array_dense_slots_resolved(keys);
    debug_assert!(index < len);
    let slot = slots.add(index);
    let previous = JSValue::from_bits((*slot).to_bits());
    if !previous.is_string() || previous.is_short_string() {
        return;
    }
    if crate::string::is_image_string(previous.as_string_ptr()) {
        return;
    }
    crate::gc::runtime_store_jsvalue_slot(
        keys as usize,
        slot as usize,
        index,
        JSValue::string_ptr(key as *mut StringHeader).bits(),
    );
}

mod records;
pub(crate) use records::{CanonicalKeys, LiveObject, SharedLayout};


/// No trie node: an owned (dictionary) list, or a list whose holder never
/// learned one. Shared with `ObjectKeys` and the shape record's packed field.
pub(crate) const NO_NODE: u32 = u32::MAX;
/// Node ids are below this: the shape record carries a node in 24 bits
/// (`ShapeRecord::keys_node`), beside the element kind's byte. An encoding
/// capacity, like the ShapeId space: ids of pruned nodes are reused, so it
/// bounds LIVE lists per agent, and running out aborts with a message, as
/// ShapeId exhaustion does.
pub(crate) const NODE_ID_LIMIT: u32 = (1 << 24) - 1;
/// The permanent empty node. Its optional backing follows the weak prune.
pub(crate) const ROOT_NODE: u32 = 0;

struct Node {
    /// Weak backing array. A published node's list is its first `len`
    /// entries; an unpublished prefix borrows a descendant's backing only for
    /// edge validation and never exposes it as its keys. 0 denotes an
    /// unbacked root or a free slot.
    addr: usize,
    published: bool,
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
    /// Element capacity of the backing this node ALLOCATED, or 0. Exactly one
    /// node per backing owns it (the list it was allocated for), so the live
    /// backing census is a sum over nodes and needs no per-backing table.
    backing_slots: u32,
    /// Keys this lineage has grown by, one receiver's append at a time, since
    /// a node on it was last REACHED by another arrival (a probe hit, a
    /// declared whole list, or a receiver that latched away from it). 0 means
    /// reached. The dictionary trigger reads it: a list no other receiver has
    /// reached is unique to the object growing it. See [`take_unique_run`].
    run: u32,
}

/// The shape-table half of a node's record (CKI S2), stored as the trie's
/// second column under the same node id: the trie walk touches only the
/// `Node` column, and only family and index operations touch this one.
#[derive(Default)]
struct NodeShapes {
    /// The node whose list first published this node's backing, or `NO_NODE`
    /// when that is this node itself. Only a tip append publishes a list on
    /// its parent's backing, so only it names another owner: the chain shares
    /// its first list's owner, and the backing's slot index has one home.
    backing_owner: u32,
    /// The descriptors of this list and, on a backing owner, its slot index.
    list: crate::object::shapes::ListShapes,
}

impl NodeShapes {
    fn vacant() -> Self {
        NodeShapes {
            backing_owner: NO_NODE,
            list: crate::object::shapes::ListShapes::default(),
        }
    }

    fn is_vacant(&self) -> bool {
        self.backing_owner == NO_NODE && self.list.is_empty()
    }

    /// Back to vacant, dropping only state that holds something.
    #[inline]
    fn clear(&mut self) {
        self.backing_owner = NO_NODE;
        if !self.list.is_empty() {
            self.list = crate::object::shapes::ListShapes::default();
        }
    }
}

/// The trie's shape column, in fixed pages that never move. An id is its
/// ordinal (page, slot); growth adds a page and copies nothing, and only the
/// pages holding an entry that owns heap memory are visited on drop.
struct PagedColumn<T> {
    pages: Vec<Box<[std::mem::MaybeUninit<T>; NODE_PAGE]>>,
    /// Nodes `0..len` are initialized; every page is full but the last.
    len: usize,
    /// Pages that hold a node whose shape state owns heap memory: only these
    /// need their nodes dropped (every other node is plain data).
    heap_pages: Vec<bool>,
}

const NODE_PAGE_SHIFT: usize = 7;
const NODE_PAGE: usize = 1 << NODE_PAGE_SHIFT;

impl<T: ShapeColumnEntry> PagedColumn<T> {
    fn with_root(root: T) -> Self {
        let mut store = PagedColumn {
            pages: Vec::new(),
            len: 0,
            heap_pages: Vec::new(),
        };
        store.push(root);
        store
    }

    #[inline]
    fn push(&mut self, node: T) {
        let slot = self.len & (NODE_PAGE - 1);
        if slot == 0 {
            self.new_page();
        }
        let page = self.pages.last_mut().expect("a page for the new node");
        page[slot].write(node);
        self.len += 1;
    }

    #[cold]
    #[optimize(size)]
    #[inline(never)]
    fn new_page(&mut self) {
        // SAFETY: an array of `MaybeUninit` needs no initialization.
        self.pages.push(unsafe { Box::new_uninit().assume_init() });
        self.heap_pages.push(false);
    }

    /// Node `id`'s shape state may now own heap memory.
    #[inline]
    fn note_heap(&mut self, id: usize) {
        self.heap_pages[id >> NODE_PAGE_SHIFT] = true;
    }

    /// # Safety
    /// `id < self.len`.
    #[inline(always)]
    unsafe fn slot(&self, id: usize) -> &T {
        self.pages
            .get_unchecked(id >> NODE_PAGE_SHIFT)
            .get_unchecked(id & (NODE_PAGE - 1))
            .assume_init_ref()
    }

    /// # Safety
    /// `id < self.len`.
    #[inline(always)]
    unsafe fn slot_mut(&mut self, id: usize) -> &mut T {
        self.pages
            .get_unchecked_mut(id >> NODE_PAGE_SHIFT)
            .get_unchecked_mut(id & (NODE_PAGE - 1))
            .assume_init_mut()
    }
}

impl<T> Drop for PagedColumn<T> {
    fn drop(&mut self) {
        for (page, &heap) in self.heap_pages.iter().enumerate() {
            if !heap {
                continue;
            }
            let first = page << NODE_PAGE_SHIFT;
            for id in first..self.len.min(first + NODE_PAGE) {
                // SAFETY: initialized and dropped exactly once.
                unsafe { std::ptr::drop_in_place(self.pages[page][id - first].as_mut_ptr()) };
            }
        }
    }
}

impl<T: ShapeColumnEntry> std::ops::Index<usize> for PagedColumn<T> {
    type Output = T;
    #[inline(always)]
    fn index(&self, id: usize) -> &T {
        assert!(id < self.len, "canonical node id out of range");
        // SAFETY: bounded by the initialized length.
        unsafe { self.slot(id) }
    }
}

impl<T: ShapeColumnEntry> std::ops::IndexMut<usize> for PagedColumn<T> {
    #[inline(always)]
    fn index_mut(&mut self, id: usize) -> &mut T {
        assert!(id < self.len, "canonical node id out of range");
        // SAFETY: bounded by the initialized length.
        unsafe { self.slot_mut(id) }
    }
}

/// An entry the paged column can drop selectively.
trait ShapeColumnEntry {
    fn owns_heap(&self) -> bool;
}

impl ShapeColumnEntry for NodeShapes {
    #[inline]
    fn owns_heap(&self) -> bool {
        self.list.owns_heap()
    }
}

/// The canonical trie for one agent.
pub(crate) struct CanonicalTable {
    nodes: Vec<Node>,
    /// The nodes' shape-table column, indexed by the same node id.
    shapes: PagedColumn<NodeShapes>,
    free: u32,
    /// `(parent node, appended-slot hash)` -> first candidate node.
    edges: FastKeyHashMap<(u32, u64), u32>,
    /// `(backing, count, node)` of the list the last append CREATED, for the
    /// publish that immediately follows it ([`take_unique_run`]).
    last_created: (usize, u32, u32),
    minted: u64,
    reaped: u64,
    #[cfg(test)]
    edge_examinations: usize,
    #[cfg(test)]
    allocated_slots: u64,
}

impl CanonicalTable {
    fn new() -> Self {
        Self {
            nodes: vec![Node {
                addr: 0,
                published: false,
                parent: NO_NODE,
                edge_hash: 0,
                next: NO_NODE,
                len: 0,
                // The empty list is vacuously all-pointer, which is what makes
                // a one-key list's answer just "is this key a heap string".
                all_ptr: true,
                backing_slots: 0,
                run: 0,
            }],
            shapes: PagedColumn::with_root(NodeShapes {
                backing_owner: ROOT_NODE,
                list: crate::object::shapes::ListShapes::default(),
            }),
            free: NO_NODE,
            // The key is `(node id, slot hash)`: a deterministic ordinal and
            // a content (or symbol-serial) hash, so occupancy and probe
            // lengths are independent of where the image or heap is laid out.
            edges: new_fast_key_hash_map(),
            last_created: (0, 0, NO_NODE),
            minted: 0,
            reaped: 0,
            #[cfg(test)]
            edge_examinations: 0,
            #[cfg(test)]
            allocated_slots: 0,
        }
    }

    /// Published lists in this table (the count the deleted address index
    /// used to hold).
    #[cfg(test)]
    fn published_count(&self) -> usize {
        self.nodes
            .iter()
            .filter(|n| n.published && n.addr != 0)
            .count()
    }

    fn alloc_node(
        &mut self,
        addr: usize,
        parent: u32,
        edge_hash: u64,
        len: u32,
        all_ptr: bool,
        published: bool,
    ) -> u32 {
        let fresh = Node {
            addr,
            published,
            parent,
            edge_hash,
            next: NO_NODE,
            len,
            all_ptr,
            backing_slots: 0,
            run: 0,
        };
        let id = if self.free != NO_NODE {
            let id = self.free;
            self.free = self.nodes[id as usize].next;
            self.nodes[id as usize] = fresh;
            // `free_nodes` left this column entry vacant.
            debug_assert!(self.shapes[id as usize].is_vacant());
            id
        } else {
            if self.nodes.len() >= NODE_ID_LIMIT as usize {
                canonical_node_ids_exhausted();
            }
            self.nodes.push(fresh);
            self.shapes.push(NodeShapes::vacant());
            (self.nodes.len() - 1) as u32
        };
        self.minted += 1;
        CANON_MINTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        CANON_LIVE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if published {
            CANON_WORDS.fetch_add(u64::from(len), std::sync::atomic::Ordering::Relaxed);
            CANON_PUBLISHED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        if parent != NO_NODE {
            let head = self.edges.entry((parent, edge_hash)).or_insert(NO_NODE);
            self.nodes[id as usize].next = *head;
            *head = id;
        }
        id
    }

    /// `id` was just published on its parent's backing by a tip append: it
    /// shares the parent's backing owner.
    #[inline]
    fn share_backing_owner(&mut self, id: u32, parent: u32) {
        let inherited = self.shapes[parent as usize].backing_owner;
        self.shapes[id as usize].backing_owner = if inherited == NO_NODE {
            parent
        } else {
            inherited
        };
    }

    /// Turn a validation-only prefix into an independently owned keys list.
    fn publish(&mut self, id: u32, addr: usize, all_ptr: bool) {
        let node = &mut self.nodes[id as usize];
        debug_assert!(!node.published);
        node.addr = addr;
        // Equal key bytes can arrive as heap strings or inline short strings.
        // The published storage, not its former witness, owns this GC fact.
        node.all_ptr = all_ptr;
        node.published = true;
        let len = node.len;
        CANON_WORDS.fetch_add(u64::from(len), std::sync::atomic::Ordering::Relaxed);
        CANON_PUBLISHED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// Record that `id`'s list was published on a backing allocated for it,
    /// with `slots` elements of capacity.
    fn own_backing(&mut self, id: u32, slots: u32) {
        self.nodes[id as usize].backing_slots = slots;
        self.shapes[id as usize].backing_owner = NO_NODE;
        CANON_BACKINGS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        CANON_BACKING_SLOTS.fetch_add(u64::from(slots), std::sync::atomic::Ordering::Relaxed);
    }

    /// `id` was just created by one receiver appending to `parent`: its run is
    /// the parent's plus one (a reached parent, or the root, restarts it).
    fn note_created(&mut self, id: u32, parent: u32) {
        let parent_run = if parent == NO_NODE || parent as usize >= self.nodes.len() {
            0
        } else {
            self.nodes[parent as usize].run
        };
        let node = &mut self.nodes[id as usize];
        node.run = parent_run.saturating_add(1);
        self.last_created = (node.addr, node.len, id);
    }

    /// Another arrival reached `id`: its lineage is not unique to one object.
    fn note_reached(&mut self, id: u32) {
        self.nodes[id as usize].run = 0;
    }

    /// Settle the nodes whose arrays did not survive a collection, without
    /// breaking any live list's path from the root.
    ///
    /// A list's array can die while a list that extends it lives on another
    /// backing: a fork, or a whole list published before an append published
    /// its prefix onto a receiver's own backing. Freeing that prefix node
    /// would cut the live list off the root, and the next walk of its keys
    /// would publish a second array for the same ordered keys. Every keys
    /// array is an identity fact of a shape, so the duplicate is a second
    /// shape for one layout, and a static ShapeId, which names the one
    /// canonical list of its keys, would then name facts no later mint of
    /// those keys can reach.
    ///
    /// So a dead node that is a prefix of a live node stays, as an
    /// unpublished witness on that live node's array: its first `len`
    /// entries are this node's list, which is all edge validation reads.
    /// Only a dead node with no live descendant is freed. One pass over the
    /// node table, two flat vectors, no index.
    fn retire_dead(&mut self, dead: &[u32]) {
        let n = self.nodes.len();
        let mut is_dead = vec![false; n];
        for &id in dead {
            if id != ROOT_NODE && (id as usize) < n && self.nodes[id as usize].addr != 0 {
                is_dead[id as usize] = true;
            }
        }
        // `witness[id]`: the array of a live list that extends dead `id`.
        let mut witness = vec![0usize; n];
        for id in 1..n {
            let node = &self.nodes[id];
            if node.addr == 0 || is_dead[id] {
                continue;
            }
            let addr = node.addr;
            let mut p = node.parent;
            // Stop at a live ancestor (its own walk covers what is above it)
            // or at one already witnessed (so was everything above it).
            while p != ROOT_NODE && p != NO_NODE && is_dead[p as usize] && witness[p as usize] == 0
            {
                witness[p as usize] = addr;
                p = self.nodes[p as usize].parent;
            }
        }
        let mut freed = Vec::with_capacity(dead.len());
        for &id in dead {
            let i = id as usize;
            if i >= n || !is_dead[i] {
                continue;
            }
            if witness[i] == 0 {
                freed.push(id);
            } else {
                self.rewitness(id, witness[i]);
            }
        }
        self.free_nodes(&freed);
    }

    /// Keep dead node `id` as an unpublished prefix of the live array `addr`.
    fn rewitness(&mut self, id: u32, addr: usize) {
        let node = &mut self.nodes[id as usize];
        if node.published {
            CANON_WORDS.fetch_sub(u64::from(node.len), std::sync::atomic::Ordering::Relaxed);
            CANON_PUBLISHED.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            node.published = false;
        }
        if node.backing_slots != 0 {
            CANON_BACKINGS.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            CANON_BACKING_SLOTS.fetch_sub(
                u64::from(node.backing_slots),
                std::sync::atomic::Ordering::Relaxed,
            );
            node.backing_slots = 0;
        }
        node.addr = addr;
        // The shape prune ran first (`DEAD_KEY_PRUNES` order): no live
        // descriptor names the dead list, and the index described its backing.
        self.shapes[id as usize].clear();
        if self.last_created.2 == id {
            self.last_created = (0, 0, NO_NODE);
        }
    }

    /// Retire a batch before reusing any id. Each edge bucket and collision
    /// candidate is visited once, including children of dead parents. The
    /// address-zero tombstone is enough to identify the dead set; no second
    /// index or per-node scan of the edge table is needed.
    fn free_nodes(&mut self, dead: &[u32]) {
        let mut retired = Vec::with_capacity(dead.len());
        for &id in dead {
            if id == ROOT_NODE || id as usize >= self.nodes.len() {
                continue;
            }
            let node = &mut self.nodes[id as usize];
            if node.addr == 0 {
                continue;
            }
            if node.published {
                CANON_WORDS.fetch_sub(u64::from(node.len), std::sync::atomic::Ordering::Relaxed);
                CANON_PUBLISHED.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            }
            if node.backing_slots != 0 {
                // Every list on a backing dies with it (the prune asks about
                // the address), so its owner dying is the backing dying.
                CANON_BACKINGS.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                CANON_BACKING_SLOTS.fetch_sub(
                    u64::from(node.backing_slots),
                    std::sync::atomic::Ordering::Relaxed,
                );
                node.backing_slots = 0;
            }
            node.addr = 0;
            // Preserve `next` until all collision chains have been filtered.
            retired.push(id);
            self.reaped += 1;
            CANON_REAPED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            CANON_LIVE.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        }
        if retired.is_empty() {
            return;
        }

        self.edges.retain(|&(parent, hash), head| {
            #[cfg(test)]
            {
                self.edge_examinations += 1;
            }
            let orphan = parent != ROOT_NODE && self.nodes[parent as usize].addr == 0;
            let mut cur = *head;
            let mut tail = NO_NODE;
            *head = NO_NODE;
            while cur != NO_NODE {
                #[cfg(test)]
                {
                    self.edge_examinations += 1;
                }
                let node = &mut self.nodes[cur as usize];
                debug_assert_eq!((node.parent, node.edge_hash), (parent, hash));
                let next = node.next;
                if orphan {
                    // A live child keeps its array and its own child edges,
                    // but must not name a parent id that can now be reused.
                    node.parent = NO_NODE;
                    node.next = NO_NODE;
                } else if node.addr != 0 {
                    node.next = NO_NODE;
                    if tail == NO_NODE {
                        *head = cur;
                    } else {
                        self.nodes[tail as usize].next = cur;
                    }
                    tail = cur;
                }
                cur = next;
            }
            *head != NO_NODE
        });

        // Only now can `next` become a free-list link. No remaining edge or
        // live child's parent can reference any of these retired slots.
        if retired.contains(&self.last_created.2) {
            self.last_created = (0, 0, NO_NODE);
        }
        for id in retired {
            self.nodes[id as usize] = Node {
                addr: 0,
                published: false,
                parent: NO_NODE,
                edge_hash: 0,
                next: self.free,
                len: 0,
                all_ptr: true,
                backing_slots: 0,
                run: 0,
            };
            self.shapes[id as usize].clear();
            self.free = id;
        }
    }
}

#[cold]
#[optimize(size)]
#[inline(never)]
fn canonical_node_ids_exhausted() -> ! {
    eprintln!(
        "perry: canonical key-list node ids exhausted ({} live lists in one agent)",
        NODE_ID_LIMIT
    );
    std::process::abort()
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
/// Live PUBLISHED lists (`live` also counts validation-only prefixes).
static CANON_PUBLISHED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Live backing arrays, and their element capacity: the storage the lists
/// above actually occupy, since one backing serves a whole growth chain.
static CANON_BACKINGS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static CANON_BACKING_SLOTS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Lists published by growing their parent's backing in place (cumulative).
static CANON_IN_PLACE_APPENDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

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
    /// The edge hash of this slot appended with attribute `entry`. A default
    /// entry (0) hashes exactly as the slot alone did before attributes lived
    /// with the keys, so every attribute-free layout keeps its edges.
    ///
    /// # Safety
    /// The operand is live.
    unsafe fn edge_hash(self, entry: u64) -> u64 {
        edge_hash_with_entry(self.slot_hash(), entry)
    }

    /// # Safety
    /// The operand is live.
    unsafe fn slot_hash(self) -> u64 {
        match self {
            Appended::Key(key) => {
                if key.is_null() {
                    return 0x4E55_4C4C_4B45_5900;
                }
                let data = crate::string::string_data(key);
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
                    None => non_string_slot_hash(v),
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
                    (Some(x), Some(y)) => crate::string::key_bytes_equal(x, y),
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

    /// A key published under stationary old metadata belongs in old storage
    /// on its first evacuation. Set the collector's existing per-object
    /// tenure bit before publication, rather than making its promotion depend
    /// on which root reaches it first. This does not pin or root the string;
    /// the normal minor still forwards every reference to its old copy.
    unsafe fn prepare_old_parent_key(self) {
        let key = match self {
            Appended::Key(key) => key,
            Appended::Slot(value) if value.is_string() => value.as_string_ptr(),
            _ => return,
        };
        if !key.is_null() && crate::arena::pointer_in_nursery(key as usize) {
            let header = crate::gc::header_from_trusted_user_ptr(key.cast()).cast_mut();
            (*header).gc_flags |= crate::gc::GC_FLAG_TENURED;
        }
    }

    /// The same slot, spelled with its text's ATOM when one exists — the one
    /// string a read site's pooled key also is (`string::intern::AtomTable`).
    ///
    /// Applied to every key this module WRITES into a list, and nowhere else:
    /// the trie validates edges by bytes, so which string object a list holds
    /// never changes which node a probe reaches, only whether a later key
    /// compare against the list can stop at pointer equality. Heap strings
    /// only — an SSO slot stays an SSO slot, so `is_pointer` (and with it the
    /// backing's all-pointer layout) is unchanged by the substitution.
    ///
    /// `h` is this slot's `edge_hash`, which for a string IS the FNV-1a hash of
    /// its bytes — the atom table's hash.
    ///
    /// # Safety
    /// The operand is live.
    unsafe fn atomized(self, h: u64) -> Self {
        match self {
            Appended::Key(key) if !key.is_null() => match crate::string::atom_for_key(key, h) {
                Some(atom) => Appended::Key(atom),
                None => self,
            },
            // An SSO short string carries its bytes in the value itself, so
            // its bits ARE its identity: equal texts are already equal words
            // and there is no heap string to replace with an atom.
            Appended::Slot(v) if v.is_short_string() => self,
            Appended::Slot(v) if v.is_string() => {
                match crate::string::atom_for_key(v.as_string_ptr(), h) {
                    Some(atom) => Appended::Slot(JSValue::string_ptr(atom as *mut StringHeader)),
                    None => self,
                }
            }
            _ => self,
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

/// The edge hash of a slot that is not a string key: a symbol hashes its
/// `SymbolHeader.id`, the symbol's own serial, under a symbol domain tag; a
/// hole (and any other constant word) hashes its bits, which are a constant.
///
/// Never the symbol's ADDRESS: that is a function of where the image or the
/// heap put it, so the edge table's occupancy, and with it every probe's
/// cost, would move with an unrelated link-layout change, and a moved symbol
/// would no longer reach its own edge. The exact check (`matches`) still
/// compares the slot words, so two symbols with one description, or a serial
/// collision, never share a list.
///
/// Out of line, so `Appended::slot_hash`'s string path stays small enough to
/// inline into every walk, as it did before symbols had their own arm.
///
/// # Safety
/// `v` is a live keys-array slot.
#[inline(never)]
unsafe fn non_string_slot_hash(v: JSValue) -> u64 {
    const SYMBOL_EDGE_DOMAIN: u64 = 0x5359_4D42_4F4C_4544;
    if v.is_pointer() {
        let sym = (v.bits() & crate::value::POINTER_MASK) as *const crate::symbol::SymbolHeader;
        if !sym.is_null() && crate::symbol::may_be_symbol_header(sym.cast()) {
            return ((*sym).id ^ SYMBOL_EDGE_DOMAIN).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        }
    }
    v.bits() ^ 0x9E37_79B9_7F4A_7C15
}

#[inline]
fn edge_hash_with_entry(hash: u64, entry: u64) -> u64 {
    if entry == 0 {
        hash
    } else {
        (hash ^ entry.wrapping_mul(0x9E37_79B9_7F4A_7C15)).rotate_left(29)
    }
}

/// The node a holder carries, validated: `Some` exactly when that node still
/// publishes this `(backing, count)`. One read of the node and three
/// compares, the same condition under which the address index this replaced
/// answered; a node pruned, rewitnessed or reused for another list since the
/// holder learned it answers `None`.
#[inline]
fn validated_node(
    t: &CanonicalTable,
    arr: *const ArrayHeader,
    count: u32,
    node: u32,
) -> Option<u32> {
    let n = t.nodes.get(node as usize)?;
    (n.published && n.addr == arr as usize && n.len == count).then_some(node)
}

/// The node id of a canonical handle, or `None` when a death prune dropped it.
#[inline]
fn node_of(t: &CanonicalTable, keys: CanonicalKeys) -> Option<u32> {
    if keys.is_empty() {
        return Some(ROOT_NODE);
    }
    validated_node(t, keys.arr, keys.count, keys.node)
}

/// Probe the trie for `parent + appended`, validating the appended slot
/// exactly.
///
/// # Safety
/// `parent` names a live canonical array (or is empty) and `appended` is live.
unsafe fn probe_node(
    t: &CanonicalTable,
    pnode: u32,
    parent_len: u32,
    appended: Appended,
    entry: u64,
    h: u64,
) -> Option<u32> {
    probe_matching_node(t, pnode, parent_len, entry, h, |stored| {
        appended.matches(stored)
    })
}

/// The existing owner validates an edge; probing never needs a newly interned
/// representation of the incoming bytes. Specialization keeps the ordinary
/// key probe unchanged and removes weak-cache custody from borrowed birth keys.
#[inline]
unsafe fn probe_matching_node(
    t: &CanonicalTable,
    pnode: u32,
    parent_len: u32,
    entry: u64,
    h: u64,
    matches: impl Fn(JSValue) -> bool,
) -> Option<u32> {
    let mut cur = *t.edges.get(&(pnode, h))?;
    while cur != NO_NODE {
        let node = &t.nodes[cur as usize];
        if node.addr != 0 && node.len == parent_len + 1 {
            let arr = node.addr as *const ArrayHeader;
            // A node's list is a prefix of its backing (published) or of a
            // descendant's (a witness): the backing holds at least `len`.
            if (*arr).length >= node.len {
                let (slots, slot_len) = crate::object::keys_array_dense_slots(arr);
                if (parent_len as usize) < slot_len {
                    #[cfg(test)]
                    canonical_keys_tests::note_slot_read();
                    let stored = JSValue::from_bits((*slots.add(parent_len as usize)).to_bits());
                    // The attribute entry is half of the edge: the same key
                    // with other attributes is a different list.
                    if matches(stored)
                        && crate::object::key_attrs::token_of_word(
                            crate::object::key_attrs::key_word(arr, parent_len),
                        ) == entry
                    {
                        return Some(cur);
                    }
                }
            }
        }
        cur = node.next;
    }
    None
}

unsafe fn probe(
    parent: CanonicalKeys,
    parent_len: u32,
    appended: Appended,
    entry: u64,
    h: u64,
) -> Option<CanonicalKeys> {
    with_table_or(None, |t| {
        let id = probe_node(t, node_of(t, parent)?, parent_len, appended, entry, h)?;
        let node = &t.nodes[id as usize];
        let hit = node.published.then_some(CanonicalKeys::new(
            node.addr as *mut ArrayHeader,
            node.len,
            id,
        ));
        if hit.is_some() {
            t.note_reached(id);
        }
        hit
    })
}

/// Find a born layout directly in the canonical trie before allocating its
/// temporary key and attribute arrays. Exact keys and entries validate every
/// edge; misses use the ordinary whole-list publication path.
///
/// # Safety
/// The caller suppresses collection while the existing canonical strings are read.
pub(crate) unsafe fn probe_born_layout(
    _proof: &SharedLayout,
    entries: &[(&str, f64)],
    attrs: &[u8],
) -> Option<CanonicalKeys> {
    if entries.is_empty() || entries.iter().any(|(key, _)| !key.is_ascii()) {
        return None;
    }
    debug_assert!(crate::gc::gc_is_suppressed());
    with_table_or(None, |t| {
        let mut parent = ROOT_NODE;
        for (i, (key, _)) in entries.iter().enumerate() {
            let bytes = key.as_bytes();
            let entry = attrs.get(i).copied().unwrap_or(0) as u64;
            let h = crate::object::key_bytes_hash(bytes.as_ptr(), bytes.len());
            let h = edge_hash_with_entry(h, entry);
            parent = probe_matching_node(t, parent, i as u32, entry, h, |stored| {
                crate::string::js_string_key_matches_bytes(stored, bytes)
            })?;
        }
        let node = &t.nodes[parent as usize];
        let hit = node.published.then_some(CanonicalKeys::new(
            node.addr as *mut ArrayHeader,
            node.len,
            parent,
        ));
        if hit.is_some() {
            t.note_reached(parent);
        }
        hit
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
/// appended, carrying attribute `entry` (`key_attrs.rs`; 0 = default). A hit
/// is O(1): one hash probe and one exact slot-and-entry check. A new
/// publication owns a copy; no intermediate unpublished prefix allocates.
///
/// # Safety
/// `parent` names a live canonical array (or is empty), `appended` is live,
/// and the caller has rooted everything it holds across the allocation this
/// may perform.
#[inline(always)]
pub(crate) unsafe fn extend_slot(
    proof: &SharedLayout,
    parent: CanonicalKeys,
    appended: Appended,
    entry: u8,
) -> CanonicalKeys {
    extend_token(proof, parent, appended, entry as u64)
}

/// [`extend_slot`] for a trie token ([`key_attrs::token_of_word`]): a key
/// re-appended from an existing list keeps its own storage slot.
unsafe fn extend_token(
    _proof: &SharedLayout,
    parent: CanonicalKeys,
    appended: Appended,
    entry: u64,
) -> CanonicalKeys {
    let h = appended.edge_hash(entry);
    let parent_len = parent.len();
    if let Some(hit) = probe(parent, parent_len, appended, entry, h) {
        return hit;
    }
    // A new list is about to be WRITTEN: it holds the atom of this key's text.
    let appended = appended.atomized(appended.slot_hash());
    appended.prepare_old_parent_key();

    // Whether the child's slots are all heap string pointers is the parent's
    // answer AND this slot's, so it is read before the allocation and never
    // re-derived by walking the list.
    // A torn-down table cannot promise an all-pointer layout, so `false` is
    // the safe default: the mask path is correct for any content.
    let parent_all_ptr = with_table_or(false, |t| {
        node_of(t, parent)
            .map(|id| t.nodes[id as usize].all_ptr)
            .unwrap_or(false)
    });
    let all_ptr = parent_all_ptr && appended.is_pointer();

    // The tip of its backing grows in place: no copy, no new array.
    if let Some(child) = append_at_tip(parent, appended, entry, h, parent_all_ptr, all_ptr) {
        return child;
    }
    // The child carries an attributes array iff some entry of it is not the
    // default: the parent's summary is exact for a canonical prefix.
    let with_attrs =
        entry != 0 || crate::object::key_attrs::keys_need_attrs(parent.as_const_ptr(), parent_len);

    // Nothing may be held across the allocation: no table borrow (a collection
    // re-enters this table through its scanner and its prune) and both
    // operands rooted, because a collection here moves them.
    let scope = crate::gc::RuntimeHandleScope::new();
    let parent_handle_keys = parent;
    let parent_handle = scope.root_raw_mut_ptr(parent.as_ptr());
    let appended_handle = match appended {
        Appended::Key(key) => scope.root_string_ptr(key),
        Appended::Slot(v) => scope.root_nanbox_u64(v.bits()),
    };
    // Canonical lists are weakly held and must use reclaimable storage. The
    // new backing starts a chain (a fork, or a tip whose backing was full),
    // so it is sized for the chain to keep growing in place.
    let capacity = backing_capacity(parent_len + 1);
    #[cfg(test)]
    try_with_table(|t| t.allocated_slots += u64::from(capacity));
    let allocate =
        || crate::object::key_attrs::alloc_canonical_key_list(capacity, all_ptr, with_attrs);
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
    let parent = parent_handle_keys.rooted_at(parent);

    // A collection during the allocation may have published this exact node
    // through another path, or pruned the parent. Re-probe before writing.
    if let Some(hit) = probe(parent, parent_len, appended, entry, h) {
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
    if with_attrs {
        #[cfg(feature = "attr-census")]
        crate::object::attr_census::note_global(if entry != 0 {
            "keys.fork_with_attrs.entry"
        } else {
            "keys.fork_with_attrs.default"
        });
        // The parent's entries, then this one: an attribute list's prefix is
        // copied with its keys, exactly like the keys themselves.
        let attrs = crate::object::key_attrs::keys_attrs(fresh);
        crate::object::key_attrs::copy_entries(parent.as_const_ptr(), 0, attrs, parent_len);
        crate::object::key_attrs::attrs_write_token(
            attrs,
            parent_len,
            entry,
            JSValue::from_bits(appended.element_word().to_bits()),
        );
    }
    (*fresh).length = parent_len + 1;
    if !all_ptr {
        // Only the mixed list needs the slot walk; the all-pointer allocator
        // already declared its layout in the header, and publishing `length`
        // after the last write is the precondition it documents.
        crate::object::gc_slots::rebuild_array_layout_from_slots(fresh);
    }
    if all_ptr && crate::arena::pointer_in_old_gen(fresh as usize) {
        for i in 0..(*fresh).length as usize {
            let slot = dst.add(i) as *const u64;
            crate::gc::runtime_write_barrier_slot(fresh as usize, slot as usize, *slot);
        }
    }
    stamp_shared(fresh);

    let node = try_with_table(|t| {
        // The parent may have been pruned while we allocated. Keep the array —
        // it is a correct list — as an orphan root: the caller still gets the
        // right content and only the edge is lost.
        let pnode = node_of(t, parent).unwrap_or(NO_NODE);
        let id = if let Some(id) = probe_node(t, pnode, parent_len, appended, entry, h) {
            t.publish(id, fresh as usize, all_ptr);
            id
        } else {
            t.alloc_node(fresh as usize, pnode, h, parent_len + 1, all_ptr, true)
        };
        t.own_backing(id, capacity);
        t.note_created(id, pnode);
        id
    })
    .unwrap_or(NO_NODE);
    CanonicalKeys::new(fresh, parent_len + 1, node)
}

/// Capacity for a new backing whose first list has `len` keys: room for the
/// chain to grow in place by half again, so a chain of N keys costs O(N)
/// copying in total.
#[inline]
fn backing_capacity(len: u32) -> u32 {
    if len <= 4 {
        4
    } else {
        len.saturating_add(len / 2)
    }
}

/// Append `appended` to `parent` IN PLACE when `parent` is the tip of its
/// backing and the backing has room. Returns `None` when it cannot — a fork
/// (the backing already grew past `parent` with a different key; the probe
/// missed), a full backing, the empty list, or a non-pointer key on an
/// all-pointer backing — and the caller starts a new backing instead.
///
/// Allocates nothing, so nothing moves: the slot is written first and the
/// header length published after it, and the collector can never observe a
/// length covering an unwritten slot. The only in-place write any backing
/// ever sees, and it lands past every published count, so no list sharing
/// the backing changes.
///
/// # Safety
/// `parent` names a live canonical list and `appended` is live.
unsafe fn append_at_tip(
    parent: CanonicalKeys,
    appended: Appended,
    entry: u64,
    h: u64,
    parent_all_ptr: bool,
    all_ptr: bool,
) -> Option<CanonicalKeys> {
    let backing = parent.as_ptr();
    if backing.is_null() {
        return None;
    }
    let parent_len = parent.len();
    if (*backing).length != parent_len || parent_len >= (*backing).capacity {
        return None;
    }
    // An all-pointer backing declares every slot a heap pointer to the
    // collector; a key that is not one forks into a mixed backing.
    if parent_all_ptr && !all_ptr {
        return None;
    }
    // A backing without attributes cannot take a non-default entry in place:
    // its front has no reserve. That forks ONCE, into a backing that carries
    // an attributes array, and the chain's later appends land here again.
    let attrs = crate::object::key_attrs::keys_attrs(backing);
    if entry != 0 && attrs.is_null() {
        return None;
    }
    if !attrs.is_null() && ((*attrs).length != parent_len || parent_len >= (*attrs).capacity) {
        return None;
    }
    let pnode = with_table_or(None, |t| node_of(t, parent))?;
    debug_assert!(
        crate::value::addr_class::try_read_tracked_gc_header(backing as usize)
            .is_some_and(|gc| (*gc.as_ptr()).gc_flags & crate::gc::GC_FLAG_SHAPE_SHARED != 0),
        "a canonical backing is shape-shared from birth"
    );
    CANON_IN_PLACE_APPENDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if !attrs.is_null() {
        // Past every published count, like the key slot: no list sharing the
        // backing sees it until the length below covers it.
        crate::object::key_attrs::attrs_write_token(
            attrs,
            parent_len,
            entry,
            JSValue::from_bits(appended.element_word().to_bits()),
        );
        #[cfg(feature = "attr-census")]
        crate::object::attr_census::note_global(if entry != 0 {
            "keys.tip_append.entry"
        } else {
            "keys.tip_append.default_on_attr_backing"
        });
    }
    // The array store helper writes the slot, notes its layout (a mixed
    // backing keeps its per-slot mask) and runs the write barrier (an old
    // backing can take a young key).
    crate::array::note_array_slot(
        backing,
        parent_len as usize,
        appended.element_word().to_bits(),
    );
    let key_array = backing;
    (*key_array).length = parent_len + 1;
    let node = try_with_table(|t| {
        let id = if let Some(id) = probe_node(t, pnode, parent_len, appended, entry, h) {
            t.publish(id, backing as usize, all_ptr);
            id
        } else {
            t.alloc_node(backing as usize, pnode, h, parent_len + 1, all_ptr, true)
        };
        t.share_backing_owner(id, pnode);
        t.note_created(id, pnode);
        id
    })
    .unwrap_or(NO_NODE);
    Some(CanonicalKeys::new(backing, parent_len + 1, node))
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
    extend_slot(proof, parent, Appended::Key(key), 0)
}

/// [`extend_key`] for a key that arrives WITH its attributes (an accessor
/// install, a literal `get`/`set`, `defineProperty` of a new key): one edge,
/// and on the tip of an attribute backing an in-place append.
///
/// # Safety
/// As [`extend_slot`].
#[inline]
pub(crate) unsafe fn extend_key_with_entry(
    proof: &SharedLayout,
    parent: CanonicalKeys,
    key: *const StringHeader,
    entry: u8,
) -> CanonicalKeys {
    extend_slot(proof, parent, Appended::Key(key), entry)
}

/// The canonical list for `keys` with the attribute entries of positions
/// `from..count` replaced by `entry_at(position, slot, current entry)` — how a
/// change to an EXISTING key's attributes (a `defineProperty` redefinition,
/// `freeze`, `seal`) is expressed. The prefix below `from` is shared as is;
/// the rest is re-appended one edge at a time, so after the first fork every
/// append lands on the new backing's tip: O(count - from), like V8 copying a
/// descriptor array.
///
/// # Safety
/// `keys` is a live list with `count` initialized slots; the caller has
/// rooted what it holds: this allocates.
pub(crate) unsafe fn rebuild_with_entries(
    proof: &SharedLayout,
    keys: crate::object::ObjectKeys,
    from: u32,
    mut entry_at: impl FnMut(u32, JSValue, u8) -> u8,
) -> CanonicalKeys {
    let count = keys.count();
    let from = from.min(count);
    #[cfg(feature = "attr-census")]
    {
        crate::object::attr_census::note_global("keys.rebuild_with_entries");
        if from + 1 == count {
            crate::object::attr_census::note_global("keys.rebuild_with_entries.last_key");
        }
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let src = scope.root_raw_mut_ptr(keys.arr());
    let prefix = canonical_prefix(proof, keys, from);
    let list = scope.root_raw_mut_ptr(prefix.as_ptr());
    let mut cur = prefix;
    for pos in from..count {
        let (slot, old) = src.with_const_ptr(|arr: *const ArrayHeader| {
            let (slots, available) = crate::object::keys_array_dense_slots(arr);
            assert!(
                (pos as usize) < available,
                "a shape's count outruns its keys"
            );
            (
                JSValue::from_bits((*slots.add(pos as usize)).to_bits()),
                crate::object::key_attrs::token_of_word(crate::object::key_attrs::key_word(
                    arr, pos,
                )),
            )
        });
        let entry = (old & !0xff) | u64::from(entry_at(pos, slot, old as u8));
        // `extend_slot` roots its operands across its own allocation.
        let next = list.with_mut_ptr(|arr: *mut ArrayHeader| {
            extend_token(proof, cur.rooted_at(arr), Appended::Slot(slot), entry)
        });
        list.set_raw_mut_ptr(next.as_ptr());
        cur = next;
    }
    list.with_mut_ptr(|arr: *mut ArrayHeader| cur.rooted_at(arr))
}

/// The canonical array for the ordered key list held in `keys[0..len]`.
///
/// Materialize only this publication. Prefix nodes borrow a weak descendant
/// witness for trie validation and never return that longer array to a caller.
/// This keeps fold storage linear in input length, while all observable lists
/// retain their own header, exact length and independent element storage.
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
    if let Some(hit) = find_canonical(proof, keys, len) {
        return hit;
    }
    canonicalize_miss(keys, len)
}

/// [`canonicalize`] for a receiver's keys as its shape hands them over: a
/// list that carries its trie node is its own proof, validated by one read
/// of the node, and returned unchanged.
///
/// # Safety
/// As [`canonicalize`].
#[inline]
pub(crate) unsafe fn canonicalize_keys(
    proof: &SharedLayout,
    keys: crate::object::ObjectKeys,
) -> CanonicalKeys {
    canonical_prefix(proof, keys, keys.count())
}

/// The canonical list of the first `len` keys of `keys`. When `keys` carries
/// its node and the prefix's node (its ancestor `count - len` parent links
/// up) is published, that node is the answer: no hash and no content read.
/// Otherwise the prefix is canonicalized from its content.
///
/// # Safety
/// As [`canonicalize`]; `len <= keys.count()`.
pub(crate) unsafe fn canonical_prefix(
    proof: &SharedLayout,
    keys: crate::object::ObjectKeys,
    len: u32,
) -> CanonicalKeys {
    debug_assert!(len <= keys.count());
    if !keys.is_null() && len != 0 {
        let hit = with_table_or(None, |t| {
            let mut id = validated_node(t, keys.arr(), keys.count(), keys.node())?;
            let mut at = keys.count();
            while at > len {
                id = t.nodes[id as usize].parent;
                let node = t.nodes.get(id as usize)?;
                if node.addr == 0 || node.len != at - 1 {
                    return None;
                }
                at -= 1;
            }
            let node = &t.nodes[id as usize];
            if !node.published {
                return None;
            }
            let addr = node.addr;
            // Exactly the arrivals the content walk reports: a prefix found
            // on another backing is reached; a list already spelled by this
            // backing at this count is the holder's own list.
            if addr != keys.arr() as usize {
                t.note_reached(id);
            }
            Some(CanonicalKeys::new(addr as *mut ArrayHeader, len, id))
        });
        if let Some(hit) = hit {
            return hit;
        }
    }
    canonicalize(proof, keys.arr(), len)
}

/// The publishing half of [`canonicalize`], after its walk missed.
unsafe fn canonicalize_miss(keys: *const ArrayHeader, len: u32) -> CanonicalKeys {
    let (slots, _) = crate::object::keys_array_dense_slots(keys);
    let all_ptr =
        (0..len).all(|i| JSValue::from_bits((*slots.add(i as usize)).to_bits()).is_string());
    // Exact, not the summary: an owned source edited in place over-reports.
    let with_attrs = crate::object::key_attrs::keys_have_entries(keys, len);
    let scope = crate::gc::RuntimeHandleScope::new();
    let src = scope.root_raw_const_ptr(keys);
    #[cfg(test)]
    try_with_table(|t| t.allocated_slots += u64::from(len));
    let (fresh, keys) = src.across_const::<ArrayHeader, _>(|| {
        crate::object::key_attrs::alloc_canonical_key_list(len, all_ptr, with_attrs)
    });
    let (slots, available) = crate::object::keys_array_dense_slots(keys);
    assert!(available >= len as usize);
    let dst = crate::array::array_elements_ptr(fresh) as *mut f64;
    // GC_STORE_AUDIT(INIT): fresh is unpublished; publish length only after
    // all elements are initialized, with no intervening GC allocation.
    for i in 0..len as usize {
        dst.add(i).write(*slots.add(i));
    }
    if with_attrs {
        let attrs = crate::object::key_attrs::keys_attrs(fresh);
        crate::object::key_attrs::copy_entries(keys, 0, attrs, len);
    }
    (*fresh).length = len;
    publish_canonical(fresh, len, all_ptr)
}

/// Canonicalize a newly constructed stationary key list by transferring its
/// backing to the trie on a miss. Registration already builds this exact
/// storage; copying it again only creates an unused old-generation array.
///
/// # Safety
/// `keys` was freshly allocated by `alloc_canonical_key_list`, contains `len`
/// initialized slots and attributes, and has never been published. The caller
/// relinquishes it and uses only the returned canonical list afterward.
pub(crate) unsafe fn canonicalize_owned(
    proof: &SharedLayout,
    keys: *mut ArrayHeader,
    len: u32,
) -> CanonicalKeys {
    if let Some(hit) = find_canonical(proof, keys, len) {
        return hit;
    }
    debug_assert!(crate::arena::pointer_in_old_gen(keys as usize));
    let (slots, available) = crate::object::keys_array_dense_slots(keys);
    assert!(available >= len as usize);
    let all_ptr =
        (0..len).all(|i| JSValue::from_bits((*slots.add(i as usize)).to_bits()).is_string());
    publish_canonical(keys, len, all_ptr)
}

/// Probe the content identity without retaining or changing the source list.
unsafe fn find_canonical(
    proof: &SharedLayout,
    keys: *const ArrayHeader,
    len: u32,
) -> Option<CanonicalKeys> {
    if keys.is_null() {
        return Some(CanonicalKeys::EMPTY);
    }
    if len == 0 {
        return Some(empty_backing(proof));
    }

    // Walk without allocating. Intermediate prefixes have no observable array:
    // their weak representative is used only to compare one edge slot.
    let hit = with_table_or(None, |t| {
        let (slots, available) = crate::object::keys_array_dense_slots(keys);
        assert!(available >= len as usize);
        let mut parent = ROOT_NODE;
        for i in 0..len {
            let slot = Appended::Slot(JSValue::from_bits((*slots.add(i as usize)).to_bits()));
            let entry = crate::object::key_attrs::token_of_word(
                crate::object::key_attrs::key_word(keys, i),
            );
            parent = probe_node(t, parent, i, slot, entry, slot.edge_hash(entry))?;
        }
        let node = &t.nodes[parent as usize];
        let hit = node.published.then_some(CanonicalKeys::new(
            node.addr as *mut ArrayHeader,
            node.len,
            parent,
        ));
        // A declared whole list is not one receiver's growth. The list this
        // very array already spells at this count is the holder's own: it
        // arrives nowhere new.
        if hit.is_some() && node.addr != keys as usize {
            t.note_reached(parent);
        }
        hit
    });
    hit
}

/// Normalize and publish an exclusively owned stationary backing. No GC
/// allocation occurs while its slots and the trie edges are installed.
unsafe fn publish_canonical(fresh: *mut ArrayHeader, len: u32, all_ptr: bool) -> CanonicalKeys {
    let dst = crate::array::array_elements_ptr(fresh) as *mut f64;
    // Every heap key of the new list is its text's atom where one exists (see
    // `Appended::atomized`). Heap string to heap string, so `all_ptr` holds.
    for i in 0..len as usize {
        let slot = Appended::Slot(JSValue::from_bits((*dst.add(i)).to_bits()));
        if let Appended::Slot(v) = slot {
            if v.is_string() && !crate::string::is_image_string(v.as_string_ptr()) {
                if let Appended::Slot(atom) = slot.atomized(slot.slot_hash()) {
                    Appended::Slot(atom).prepare_old_parent_key();
                    // GC_STORE_AUDIT(INIT): `fresh` is unpublished; its length
                    // already covers the initialized slots; no GC allocation intervenes.
                    *dst.add(i) = f64::from_bits(atom.bits());
                }
            }
        }
    }
    if !all_ptr {
        crate::object::gc_slots::rebuild_array_layout_from_slots(fresh);
    }
    if all_ptr && crate::arena::pointer_in_old_gen(fresh as usize) {
        for i in 0..(*fresh).length as usize {
            let slot = dst.add(i) as *const u64;
            crate::gc::runtime_write_barrier_slot(fresh as usize, slot as usize, *slot);
        }
    }
    stamp_shared(fresh);
    with_table_or(CanonicalKeys::new(fresh, len, NO_NODE), |t| {
        // Allocation may have moved/pruned witnesses. Walk again with the
        // rooted source now copied into fresh; no table borrow spans a GC.
        let mut parent = ROOT_NODE;
        let mut prefix_all_ptr = true;
        for i in 0..len {
            let slot = Appended::Slot(JSValue::from_bits((*dst.add(i as usize)).to_bits()));
            let entry = crate::object::key_attrs::token_of_word(
                crate::object::key_attrs::key_word(fresh, i),
            );
            let h = slot.edge_hash(entry);
            prefix_all_ptr &= slot.is_pointer();
            parent = match probe_node(t, parent, i, slot, entry, h) {
                Some(id) => id,
                None => t.alloc_node(fresh as usize, parent, h, i + 1, prefix_all_ptr, false),
            };
        }
        if t.nodes[parent as usize].published {
            let node = &t.nodes[parent as usize];
            CanonicalKeys::new(node.addr as *mut ArrayHeader, node.len, parent)
        } else {
            t.publish(parent, fresh as usize, all_ptr);
            t.own_backing(parent, len);
            CanonicalKeys::new(fresh, len, parent)
        }
    })
}

/// The non-null spelling of the empty list required by class keys globals.
/// It belongs to the trie's root, just as every nonempty backing belongs to
/// its node. The root's backing is weak and is cleared by the same full-trace
/// prune; there is no separate singleton cache or immortal allocation.
pub(crate) unsafe fn empty_backing(_proof: &SharedLayout) -> CanonicalKeys {
    if let Some(addr) = with_table_or(None, |t| {
        let root = &t.nodes[ROOT_NODE as usize];
        root.published.then_some(root.addr)
    }) {
        return CanonicalKeys::new(addr as *mut ArrayHeader, 0, ROOT_NODE);
    }
    let fresh = crate::array::js_array_alloc_canonical_keys(0, true, false);
    stamp_shared(fresh);
    with_table_or(CanonicalKeys::new(fresh, 0, ROOT_NODE), |t| {
        // Allocation can collect, so resolve the root again after it.
        if !t.nodes[ROOT_NODE as usize].published {
            t.publish(ROOT_NODE, fresh as usize, true);
            t.own_backing(ROOT_NODE, 0);
        }
        CanonicalKeys::new(
            t.nodes[ROOT_NODE as usize].addr as *mut ArrayHeader,
            0,
            ROOT_NODE,
        )
    })
}

/// The canonical list for `keys` without position `remove`, every other
/// key's attribute entry carried: a delete from a shared list that carries
/// attributes. The prefix below `remove` is shared as is.
///
/// # Safety
/// As [`rebuild_with_entries`]; `remove < keys.count()`.
pub(crate) unsafe fn rebuild_removing(
    proof: &SharedLayout,
    keys: crate::object::ObjectKeys,
    remove: u32,
) -> CanonicalKeys {
    let count = keys.count();
    debug_assert!(remove < count);
    #[cfg(feature = "attr-census")]
    crate::object::attr_census::note_global("keys.rebuild_removing");
    let scope = crate::gc::RuntimeHandleScope::new();
    let src = scope.root_raw_mut_ptr(keys.arr());
    let prefix = canonical_prefix(proof, keys, remove);
    let list = scope.root_raw_mut_ptr(prefix.as_ptr());
    let mut cur = prefix;
    for pos in remove + 1..count {
        let (slot, entry) = src.with_const_ptr(|arr: *const ArrayHeader| {
            let (slots, available) = crate::object::keys_array_dense_slots(arr);
            assert!(
                (pos as usize) < available,
                "a shape's count outruns its keys"
            );
            (
                JSValue::from_bits((*slots.add(pos as usize)).to_bits()),
                crate::object::key_attrs::token_of_word(crate::object::key_attrs::key_word(
                    arr, pos,
                )),
            )
        });
        // `extend_slot` roots its operands across its own allocation.
        let next = list.with_mut_ptr(|arr: *mut ArrayHeader| {
            extend_token(proof, cur.rooted_at(arr), Appended::Slot(slot), entry)
        });
        list.set_raw_mut_ptr(next.as_ptr());
        cur = next;
    }
    list.with_mut_ptr(|arr: *mut ArrayHeader| cur.rooted_at(arr))
}

/// Post-trace death prune. A node whose array did not survive is dropped,
/// unless a live list extends it: then it stays as that list's unpublished
/// prefix ([`CanonicalTable::retire_dead`]), so no live list is ever cut off
/// the root.
#[cold]
pub(crate) fn prune_dead_canonical_keys(is_dead_owner: &dyn Fn(usize) -> bool) {
    // Snapshot first, then ask. `is_dead_owner` is a collector predicate this
    // module does not own, and calling it under the table borrow would make a
    // future re-entrant reader a panic rather than a slow path.
    let live: Vec<(u32, usize)> = CANONICAL_KEYS
        .try_with(|t| {
            let t = t.borrow();
            (0..t.nodes.len())
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
        if dead.contains(&ROOT_NODE) {
            let root = &mut t.nodes[ROOT_NODE as usize];
            root.addr = 0;
            root.published = false;
            CANON_PUBLISHED.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            CANON_BACKINGS.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        }
        t.retire_dead(&dead);
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

/// The unique run of `keys` if the append that just produced it CREATED it,
/// else 0 — and consumes the answer, so only the publish that follows the
/// append sees it.
///
/// This is the dictionary trigger's signal: how many keys this lineage grew
/// by, one receiver's append at a time, since another arrival last reached a
/// node on it. Arrivals the trie observes are probe hits, declared whole
/// lists, and receivers that latched away ([`note_latched_away`]). A second
/// receiver that follows a lineage through the TRANSITION CACHE is not
/// observed (that path never reaches the trie), so a lineage shared only that
/// way reads as unique past its last observed arrival; the latch that
/// follows marks it reached, which bounds how many followers can latch.
pub(crate) fn take_unique_run(keys: crate::object::ObjectKeys) -> u32 {
    with_table_or(0, |t| {
        let (addr, count, id) = t.last_created;
        if addr == 0 || addr != keys.arr() as usize || count != keys.count() {
            return 0;
        }
        t.last_created = (0, 0, NO_NODE);
        let node = &t.nodes[id as usize];
        if node.addr == addr && node.len == count {
            node.run
        } else {
            0
        }
    })
}

mod stats;
mod views;

pub(crate) use stats::{
    canonical_element_words, canonical_stats, canonical_storage_stats, canonical_trie_stats,
};
#[cfg(test)]
pub(crate) use stats::{node_publishes_for_test, published_lists_for_test, reset_for_test};
pub(crate) use views::{
    for_each_list_shapes, node_publishes, note_latched_away, published_node_of,
    with_backing_shapes, with_list_shapes,
};

#[cfg(test)]
#[path = "canonical_keys_tests.rs"]
mod canonical_keys_tests;

#[cfg(test)]
#[path = "canonical_keys_backing_tests.rs"]
mod canonical_keys_backing_tests;
