//! What the canonical table hands out and accepts: `CanonicalKeys` (the key a
//! list is identified by, and the node that owns it), `LiveObject` (a rooted
//! address the GC vouches for), and `SharedLayout` (the proof that a key list
//! describes a layout rather than one receiver's private list). These are types
//! and constructors, not trie storage.
use super::*;

/// A canonical key list this table owns: a backing array and the list's own
/// key count, a prefix of that backing.
///
/// The enforcement half of the funnel, and the reason it is a type rather
/// than a comment: only this module can mint one, every producer of a keys
/// list must accept one. The backing's header length is the length of the
/// longest list on it and says nothing about this one; [`Self::len`] is the
/// list's count.
///
/// **Invariant (CKI S2): a node outlives every live holder of its array.** The
/// trie frees or rewitnesses a node only in the post-trace prune, and only
/// when the collector reports its array dead; a holder that still reaches the
/// array keeps it alive, so the node it carries still publishes its list. The
/// shape mint therefore trusts a carried node (debug builds assert it), and
/// the dead-key shape prune runs before the trie prune, so no record names a
/// freed node.
///
/// The handle also carries the list's trie NODE, its identity in this agent's
/// table (CKI S1). The node is the list's owner: every consumer asks the node
/// it was handed, validated by one exact read, instead of looking the list up
/// by its address. Equality stays the list's, `(backing, count)`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CanonicalKeys {
    pub(super) arr: *mut ArrayHeader,
    pub(super) count: u32,
    pub(super) node: u32,
}

impl PartialEq for CanonicalKeys {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.arr == other.arr && self.count == other.count
    }
}

impl Eq for CanonicalKeys {}

impl CanonicalKeys {
    /// The empty list without a backing. Class globals use `empty_backing`
    /// for their non-null ABI spelling; both lists extend from the trie root.
    pub(crate) const EMPTY: CanonicalKeys = CanonicalKeys {
        arr: std::ptr::null_mut(),
        count: 0,
        node: ROOT_NODE,
    };

    #[inline]
    pub(super) fn new(arr: *mut ArrayHeader, count: u32, node: u32) -> Self {
        CanonicalKeys { arr, count, node }
    }

    /// This handle, rebuilt from the array the caller held rooted across an
    /// allocation: the array's spelling may have changed, the list (its
    /// content, count and node) cannot have.
    #[inline]
    pub(crate) fn rooted_at(self, arr: *mut ArrayHeader) -> Self {
        CanonicalKeys { arr, ..self }
    }

    /// A handle on an array no trie published, for a test that drives the
    /// collector over a hand-built backing. It names no node.
    #[cfg(test)]
    pub(crate) fn unlinked_for_test(arr: *mut ArrayHeader, count: u32) -> Self {
        CanonicalKeys::new(arr, count, NO_NODE)
    }

    /// The list's trie node in this agent (`NO_NODE` only for a list a
    /// torn-down table could not record).
    #[inline]
    pub(crate) fn node(self) -> u32 {
        self.node
    }

    /// The backing array.
    #[inline]
    pub(crate) fn as_ptr(self) -> *mut ArrayHeader {
        self.arr
    }

    #[inline]
    pub(crate) fn as_const_ptr(self) -> *const ArrayHeader {
        self.arr as *const ArrayHeader
    }

    #[cfg(test)]
    #[inline]
    pub(crate) fn addr(self) -> usize {
        self.arr as usize
    }

    #[inline]
    pub(crate) fn is_empty(self) -> bool {
        self.arr.is_null()
    }

    /// This list as a receiver's keys: the backing and the list's own count.
    #[inline]
    pub(crate) fn view(self) -> crate::object::ObjectKeys {
        crate::object::ObjectKeys::with_node(self.arr, self.count, self.node)
    }

    /// The list's key count — a prefix of the backing, never the backing's
    /// header length.
    #[inline]
    pub(crate) fn len(self) -> u32 {
        self.count
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
    /// No unrooted object crosses this call.
    #[inline]
    pub(crate) fn none() -> Self {
        LiveObject(std::ptr::null_mut())
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
    _private: (),
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
        if obj.is_null() || crate::object::overflow::has_overflow(obj) {
            return None;
        }
        Some(SharedLayout { _private: () })
    }

    /// The shape cache's entries are shared layouts by construction: they are
    /// keyed by a STATIC shape id and handed to every receiver of that shape,
    /// so there is no receiver whose kind could make them private. The one
    /// place the proof is not a kind check, named so it is auditable rather
    /// than implicit.
    #[inline]
    pub(crate) fn shape_cache_entry() -> Self {
        SharedLayout { _private: () }
    }
}
