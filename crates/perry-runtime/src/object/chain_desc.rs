//! ChainDesc (DYNP-READ item 4, design /root/DYNP/chain1/DESIGN-CHAIN.md §2-§3).
//!
//! One `ChainNode` per marked prototype OBJECT, reached from the cold extension
//! of its meta record. Each node owns one immutable `ChainDesc`: the COMPLETE
//! key -> nearest-holder map of the node and its ancestors, up to the null
//! prototype or the first `boundary` node (Proxy, exotic, unsealed builtin),
//! whose misses prove nothing. A read of an absent or inherited key does one
//! exact lookup here instead of walking the chain.
//!
//! Built eagerly at establishment and on every structural mutation (rebuild of
//! the node, then its subtree in pre-order, ancestor before descendant). A
//! read never writes a desc. No epochs, no validity flags, no caches: the desc
//! is a structural fact of the prototype graph. Keys are compared by full key
//! bits; the hash only places an entry. Values are never copied: an entry
//! names the holder node and a location.
//!
//! This file is the object-model-free core (structures, build, lookup, tree
//! links, pre-order rebuild, freeing); the glue that lists a prototype's own
//! keys and the read-path swap live beside the object code.
#![allow(dead_code)]

use std::alloc::{alloc_zeroed, dealloc, Layout};
use std::ptr;

/// What a desc entry says about the holder's own key.
pub(crate) mod act {
    /// Data in an inline list slot: `loc` = list position.
    pub(crate) const DATA_INLINE: u8 = 1;
    /// Data in a lane slot of the given representation (I32/F64/Any): `loc` = list position.
    pub(crate) const DATA_LANE_I32: u8 = 2;
    pub(crate) const DATA_LANE_F64: u8 = 3;
    pub(crate) const DATA_LANE_ANY: u8 = 4;
    /// Data in an overflow entry: `loc` = entry number.
    pub(crate) const DATA_OVF: u8 = 5;
    /// Accessor pair: the getter runs with the ORIGINAL receiver.
    pub(crate) const ACCESSOR: u8 = 6;
    /// A non-atom text key: a hash match is resolved by the exact byte
    /// compare on the cold path; never a hit by hash alone.
    pub(crate) const TEXT: u8 = 7;
}

/// Overflow-location bit in `ChainEntry::loc`.
pub(crate) const LOC_OVF: u32 = 1 << 31;
/// The empty bucket's key word: no property key has all-zero bits.
pub(crate) const EMPTY_KEY: u64 = 0;
/// A probe chain longer than this makes the build take the next table size.
/// Derived from the complete key set at build time, never from reads.
pub(crate) const MAX_PROBE: u32 = 4;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct ChainEntry {
    pub(crate) key_bits: u64,
    pub(crate) holder: *mut ChainNode,
    pub(crate) loc: u32,
    pub(crate) hash32: u32,
    /// The holder's one-load slot for a data hit (`NO_LOAD` = read through
    /// the holder's record), derived from the holder's key index at build.
    pub(crate) load: u32,
    pub(crate) act: u8,
    /// The holder's key is an enumerable string key (for-in visible), read
    /// from the holder's own attributes at build.
    pub(crate) enumerable: bool,
}

const EMPTY_ENTRY: ChainEntry = ChainEntry {
    key_bits: EMPTY_KEY,
    holder: ptr::null_mut(),
    loc: 0,
    hash32: 0,
    load: 0,
    act: 0,
    enumerable: false,
};

/// Immutable once published. `entries[mask + 1]` trail the header.
#[repr(C)]
pub(crate) struct ChainDesc {
    pub(crate) mask: u32,
    pub(crate) count: u32,
    /// Enumerable string keys on every level of this map (this node and its
    /// ancestors up to `boundary`), shadowed ones included; 0 with a null
    /// `boundary` proves a for-in past the receiver emits nothing. Computed
    /// at build from the holders' own attributes, never by a read.
    pub(crate) enum_count: u32,
    /// First node (this one or an ancestor) whose misses prove nothing; null
    /// when the map is complete up to the null prototype.
    pub(crate) boundary: *mut ChainNode,
    /// The node this desc belongs to. The prototype's owned-state slot names
    /// the desc (one level less on every read); node users go through here.
    pub(crate) node: *mut ChainNode,
    entries: [ChainEntry; 0],
}

/// One per marked prototype object; malloc'd, stable address.
#[repr(C)]
pub(crate) struct ChainNode {
    /// The prototype's object bits. Rewritten by the GC rewrite passes like
    /// the shape identity words; not a root (the object owns the node).
    pub(crate) obj: u64,
    pub(crate) desc: *const ChainDesc,
    pub(crate) parent: *mut ChainNode,
    pub(crate) first_child: *mut ChainNode,
    pub(crate) next_sibling: *mut ChainNode,
    pub(crate) prev_sibling: *mut ChainNode,
    /// The node itself is a boundary (Proxy, exotic read receiver, deferred
    /// namespace, typed-array prototype, unsealed builtin, native alias).
    pub(crate) exotic: bool,
    /// The [[Prototype]] link of this object leads to something that is not
    /// a node (unknown link): treated as a boundary just past this node.
    pub(crate) open_above: bool,
    /// The agent's list of all nodes (GC rewrite of `obj`, dead-owner prune).
    pub(crate) all_next: *mut ChainNode,
    pub(crate) all_prev: *mut ChainNode,
    /// Open above because its [[Prototype]] is an object with no node yet:
    /// on the glue's waiting list (in it exactly while this is set), so that
    /// object's establishment adopts it.
    pub(crate) waiting: bool,
    pub(crate) wait_next: *mut ChainNode,
    pub(crate) wait_prev: *mut ChainNode,
}

/// One own key of a node, listed by the object-model glue.
#[derive(Clone, Copy, Debug)]
pub(crate) struct OwnKey {
    pub(crate) key_bits: u64,
    pub(crate) hash: u64,
    pub(crate) loc: u32,
    pub(crate) load: u32,
    pub(crate) act: u8,
    /// An enumerable string key (for-in visible).
    pub(crate) enumerable: bool,
}

/// The answer of one lookup.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Found {
    /// Nearest holder of the key.
    Hit(ChainEntry),
    /// A TEXT entry with the same hash: the exact compare decides (cold).
    Text(ChainEntry),
    /// Not on the chain below `boundary`; null boundary = proven absent.
    Miss(*mut ChainNode),
}

fn desc_layout(buckets: usize) -> Layout {
    let size = std::mem::size_of::<ChainDesc>() + buckets * std::mem::size_of::<ChainEntry>();
    Layout::from_size_align(size, std::mem::align_of::<ChainDesc>().max(8))
        .expect("chain desc layout")
}

impl ChainDesc {
    #[inline(always)]
    pub(crate) unsafe fn entries(d: *const ChainDesc) -> *const ChainEntry {
        ptr::addr_of!((*d).entries) as *const ChainEntry
    }

    /// Bytes this desc occupies (fan-out report).
    pub(crate) unsafe fn bytes(d: *const ChainDesc) -> usize {
        if d.is_null() {
            0
        } else {
            desc_layout((*d).mask as usize + 1).size()
        }
    }

    /// The read leaf: one exact lookup. Typically one probe.
    #[inline(always)]
    pub(crate) unsafe fn lookup(d: *const ChainDesc, key_bits: u64, hash: u64) -> Found {
        let mask = (*d).mask as usize;
        let es = Self::entries(d);
        let mut b = hash as usize & mask;
        loop {
            let e = &*es.add(b);
            // A site key is an atom, a TEXT entry's key is not: equal bits
            // are a hit (a TEXT entry so named walks, which is exact).
            if e.key_bits == key_bits {
                return if e.act != act::TEXT {
                    Found::Hit(*e)
                } else {
                    Found::Text(*e)
                };
            }
            if e.key_bits == EMPTY_KEY {
                return Found::Miss((*d).boundary);
            }
            if e.act == act::TEXT && e.hash32 == hash as u32 {
                return Found::Text(*e);
            }
            b = (b + 1) & mask;
        }
    }

    unsafe fn free(d: *const ChainDesc) {
        if !d.is_null() {
            dealloc(d as *mut u8, desc_layout((*d).mask as usize + 1));
        }
    }
}

/// Place `e` in a table of `mask + 1` buckets unless its key is there.
/// Returns the probe length, or None if the key was already present.
unsafe fn place(es: *mut ChainEntry, mask: usize, e: ChainEntry) -> Option<u32> {
    let mut b = e.hash32 as usize & mask;
    let mut n = 1;
    loop {
        let slot = &mut *es.add(b);
        if slot.key_bits == EMPTY_KEY {
            *slot = e;
            return Some(n);
        }
        if slot.key_bits == e.key_bits {
            return None;
        }
        b = (b + 1) & mask;
        n += 1;
    }
}

/// Build the desc of `node` from its parent's desc and its own keys.
/// Own keys win; a key present in the parent's desc and not owned keeps the
/// parent's holder. A hole in the own list is not passed in (absence at N).
pub(crate) unsafe fn build(node: *mut ChainNode, own: &[OwnKey]) -> *const ChainDesc {
    let parent = (*node).parent;
    let pd = if parent.is_null() {
        ptr::null()
    } else {
        (*parent).desc
    };
    let boundary = if (*node).exotic {
        node
    } else if (*node).open_above {
        // Misses past this node prove nothing; own keys still answer.
        node
    } else if pd.is_null() {
        ptr::null_mut()
    } else {
        (*pd).boundary
    };
    let inherited: &[ChainEntry] = if pd.is_null() || (*node).exotic {
        &[]
    } else {
        std::slice::from_raw_parts(ChainDesc::entries(pd), (*pd).mask as usize + 1)
    };
    // An exotic node's own keys do not answer either: its [[Get]] is not an
    // ordinary own lookup. Its desc is empty with itself as boundary.
    let own: &[OwnKey] = if (*node).exotic { &[] } else { own };
    let upper = own.len()
        + if pd.is_null() {
            0
        } else {
            (*pd).count as usize
        };
    let mut buckets = (upper * 2).next_power_of_two().max(4);
    // Equal full hashes cannot be separated by size: past this cap a longer
    // chain is accepted (still exact, the key bits decide).
    let grow_cap = (upper * 16).next_power_of_two().max(64);
    'size: loop {
        let layout = desc_layout(buckets);
        let d = alloc_zeroed(layout) as *mut ChainDesc;
        assert!(!d.is_null(), "chain desc allocation");
        (*d).mask = (buckets - 1) as u32;
        (*d).boundary = boundary;
        (*d).node = node;
        let es = ChainDesc::entries(d) as *mut ChainEntry;
        let mask = buckets - 1;
        let mut count = 0u32;
        let mut ok = true;
        let entries = own
            .iter()
            .map(|k| ChainEntry {
                key_bits: k.key_bits,
                holder: node,
                loc: k.loc,
                hash32: k.hash as u32,
                load: k.load,
                act: k.act,
                enumerable: k.enumerable,
            })
            .chain(
                inherited
                    .iter()
                    .copied()
                    .filter(|e| e.key_bits != EMPTY_KEY),
            );
        for e in entries {
            debug_assert!(e.key_bits != EMPTY_KEY);
            match place(es, mask, e) {
                Some(n) if n > MAX_PROBE && buckets < grow_cap => {
                    ok = false;
                    break;
                }
                Some(_) => count += 1,
                None => {}
            }
        }
        if !ok {
            dealloc(d as *mut u8, layout);
            buckets *= 2;
            continue 'size;
        }
        (*d).count = count;
        (*d).enum_count = own.iter().filter(|k| k.enumerable).count() as u32
            + if pd.is_null() || (*node).exotic {
                0
            } else {
                (*pd).enum_count
            };
        return d;
    }
}

pub(crate) unsafe fn new_node(obj: u64, exotic: bool) -> *mut ChainNode {
    Box::into_raw(Box::new(ChainNode {
        obj,
        desc: ptr::null(),
        parent: ptr::null_mut(),
        first_child: ptr::null_mut(),
        next_sibling: ptr::null_mut(),
        prev_sibling: ptr::null_mut(),
        exotic,
        open_above: false,
        all_next: ptr::null_mut(),
        all_prev: ptr::null_mut(),
        waiting: false,
        wait_next: ptr::null_mut(),
        wait_prev: ptr::null_mut(),
    }))
}

/// Link `node` (currently unlinked) under `parent` (null = null prototype).
pub(crate) unsafe fn link(node: *mut ChainNode, parent: *mut ChainNode) {
    debug_assert!((*node).parent.is_null() && (*node).prev_sibling.is_null());
    (*node).parent = parent;
    if parent.is_null() {
        return;
    }
    let first = (*parent).first_child;
    (*node).next_sibling = first;
    (*node).prev_sibling = ptr::null_mut();
    if !first.is_null() {
        (*first).prev_sibling = node;
    }
    (*parent).first_child = node;
}

pub(crate) unsafe fn unlink(node: *mut ChainNode) {
    let parent = (*node).parent;
    let prev = (*node).prev_sibling;
    let next = (*node).next_sibling;
    if !prev.is_null() {
        (*prev).next_sibling = next;
    } else if !parent.is_null() {
        (*parent).first_child = next;
    }
    if !next.is_null() {
        (*next).prev_sibling = prev;
    }
    (*node).parent = ptr::null_mut();
    (*node).prev_sibling = ptr::null_mut();
    (*node).next_sibling = ptr::null_mut();
}

/// Pre-order successor of `n` within the subtree rooted at `root`, no stack.
unsafe fn next_pre_order(n: *mut ChainNode, root: *mut ChainNode) -> *mut ChainNode {
    if !(*n).first_child.is_null() {
        return (*n).first_child;
    }
    let mut cur = n;
    while cur != root {
        if !(*cur).next_sibling.is_null() {
            return (*cur).next_sibling;
        }
        cur = (*cur).parent;
    }
    ptr::null_mut()
}

/// Rebuild `root` and its whole subtree, ancestor before descendant; each
/// node's old desc is freed at its swap. `own_keys` lists a node's own keys
/// (object-model glue). Returns (nodes rebuilt, desc bytes published).
pub(crate) unsafe fn rebuild_subtree(
    root: *mut ChainNode,
    own_keys: impl FnMut(*mut ChainNode, &mut Vec<OwnKey>),
) -> (usize, usize) {
    rebuild_subtree_publishing(root, own_keys, |_, _| {})
}

/// [`rebuild_subtree`], handing each node's fresh desc to `publish` before
/// the old one is freed (the glue rewrites the word that names it).
pub(crate) unsafe fn rebuild_subtree_publishing(
    root: *mut ChainNode,
    mut own_keys: impl FnMut(*mut ChainNode, &mut Vec<OwnKey>),
    mut publish: impl FnMut(*mut ChainNode, *const ChainDesc),
) -> (usize, usize) {
    let mut keys = Vec::new();
    let mut n = root;
    let mut nodes = 0;
    let mut bytes = 0;
    while !n.is_null() {
        keys.clear();
        own_keys(n, &mut keys);
        let fresh = build(n, &keys);
        let old = (*n).desc;
        (*n).desc = fresh;
        publish(n, fresh);
        ChainDesc::free(old);
        nodes += 1;
        bytes += ChainDesc::bytes(fresh);
        n = next_pre_order(n, root);
    }
    (nodes, bytes)
}

/// A prototype died: free its node and desc. Its descendants are dead too
/// (they reach it); the GC prune frees each, in any order, so the links of
/// a dying node are cleared without touching freed neighbours' descs.
pub(crate) unsafe fn free_node(node: *mut ChainNode) {
    unlink(node);
    let mut c = (*node).first_child;
    while !c.is_null() {
        let next = (*c).next_sibling;
        (*c).parent = ptr::null_mut();
        (*c).prev_sibling = ptr::null_mut();
        (*c).next_sibling = ptr::null_mut();
        c = next;
    }
    ChainDesc::free((*node).desc);
    drop(Box::from_raw(node));
}

#[cfg(test)]
#[path = "chain_desc_tests.rs"]
mod tests;
