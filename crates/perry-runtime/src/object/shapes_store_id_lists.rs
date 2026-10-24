/// A spilled id list: the ids, plus an `id -> index` map built once the list
/// is large enough for a linear scan to cost more than a hash probe.
///
/// The index is what makes `remove_unordered`, `contains` and `position` O(1)
/// on the lists that actually get long. Below [`SPILL_INDEX_MIN`] it stays
/// empty and every operation is the linear scan it always was, because for a
/// handful of entries the scan is a single cache line and the map is not.
#[derive(Clone, Debug, Default)]
pub(super) struct SpillList {
    ids: Vec<u32>,
    /// Empty while `ids.len() < SPILL_INDEX_MIN`; complete above it.
    ///
    /// `PtrHasher` (#8125) is the right hasher here for the same reason it is
    /// on the maps around it: shape ids come from a monotonic counter, so the
    /// key is a small dense integer and the avalanche step is what keeps every
    /// one of them off bucket 0.
    pos: crate::fast_hash::PtrHashMap<u32, u32>,
}

/// Where the index starts paying. Measured shape of the problem: `families`
/// lists reach 514,030 entries on a claude-code reply while `by_facts` lists
/// are length 1, so anything in the low tens is far below the case that hurts
/// and far above the case where the map would be pure overhead.
const SPILL_INDEX_MIN: usize = 32;

impl SpillList {
    #[inline]
    fn indexed(&self) -> bool {
        !self.pos.is_empty()
    }

    /// Build the index if the list has just crossed the threshold. Called
    /// after every growth, so the map exists from the first entry past it.
    #[inline]
    fn maybe_build_index(&mut self) {
        if self.pos.is_empty() && self.ids.len() >= SPILL_INDEX_MIN {
            self.pos.reserve(self.ids.len());
            for (i, &id) in self.ids.iter().enumerate() {
                self.pos.insert(id, i as u32);
            }
        }
    }

    /// Position of `id`, O(1) when indexed and a counted linear scan below the
    /// threshold.
    #[inline]
    fn position(&self, id: u32) -> Option<usize> {
        if self.indexed() {
            return self.pos.get(&id).map(|&i| i as usize);
        }
        note_scan(self.ids.len());
        self.ids.iter().position(|&x| x == id)
    }

    #[inline]
    fn push(&mut self, id: u32) {
        let i = self.ids.len();
        self.ids.push(id);
        if self.indexed() {
            self.pos.insert(id, i as u32);
        } else {
            self.maybe_build_index();
        }
    }

    /// ORDER-PRESERVING removal, for a list whose order is load-bearing.
    /// O(n) in the tail by construction — that is what "preserve the order"
    /// costs — and it reindexes the shifted suffix.
    fn remove_ordered(&mut self, id: u32) -> Option<usize> {
        let pos = self.position(id)?;
        let moved = self.ids.len() - 1 - pos;
        note_removal(moved);
        self.ids.remove(pos);
        if self.indexed() {
            self.pos.remove(&id);
            for (i, &other) in self.ids.iter().enumerate().skip(pos) {
                self.pos.insert(other, i as u32);
            }
        }
        Some(pos)
    }

    /// UNORDERED removal: the last element takes the removed one's slot.
    /// Moves ONE element regardless of position, which is the whole point —
    /// the measured removals sit at position ~0.31 of a list up to 514,030
    /// long, so `Vec::remove` was shifting essentially the entire list every
    /// time.
    ///
    /// What this does NOT claim: that the memmove explains the bimodal turn
    /// CPU. On perrymaster one draw moved 335 GB and was as fast as a draw
    /// that moved 16 GB, so bytes moved is necessary but not sufficient for
    /// the slow mode. This removes work that is unambiguously wasted; how much
    /// TIME it removes is the A/B's to say.
    fn remove_unordered(&mut self, id: u32) -> Option<usize> {
        let pos = self.position(id)?;
        note_removal(if pos + 1 == self.ids.len() { 0 } else { 1 });
        let last = self.ids.len() - 1;
        self.ids.swap_remove(pos);
        if self.indexed() {
            self.pos.remove(&id);
            if pos != last {
                // The element that was last now lives at `pos`.
                self.pos.insert(self.ids[pos], pos as u32);
            }
        }
        Some(pos)
    }

    #[inline]
    fn replace(&mut self, old: u32, new: u32) -> bool {
        let Some(pos) = self.position(old) else {
            return false;
        };
        self.ids[pos] = new;
        if self.indexed() {
            self.pos.remove(&old);
            self.pos.insert(new, pos as u32);
        }
        true
    }
}

/// A compact list of descriptor ids: up to three inline, then a spilled
/// `Vec` with an `id -> index` map (see [`SpillList`]). Sized so a family-index
/// bucket is `(u64, IdList)` = 24 bytes.
///
/// # Order
/// Order is meaningful **for `by_facts` only**: [`IdList::push_front`] is how
/// an installed process-global id becomes the canonical answer for exact-facts
/// interning ahead of an equivalent local id (`install_external_shape_id`), and
/// that list is read first-wins. `families` is NOT order-sensitive: its only
/// order-touching reader is the "one descriptor stands for the family" choice
/// in the two rekey walks, which breaks on the first carrier and otherwise
/// takes any present member — and the chosen descriptor feeds exactly one
/// expression, `old_carrier || cache_carrier`, whose value is the same for
/// every carrier and the same for every non-carrier. The outcome is a function
/// of the SET, not of the order.
///
/// That asymmetry is why removal comes in two flavours:
/// [`IdList::remove_ordered`] for `by_facts` and [`IdList::remove_unordered`]
/// for `families`. **The caller declares the contract**, because the caller is
/// the one that knows whether its order is load-bearing; a single `remove` that
/// guessed would be the bug.
#[derive(Clone, Debug)]
pub(super) enum IdList {
    Inline { len: u8, ids: [u32; 3] },
    // The `Box` is the point: an inline `SpillList` is far wider and would make
    // every bucket pay for it; the spill is the rare case, so its extra
    // indirection is cheaper than those bytes on every family.
    Spill(Box<SpillList>),
}

const _: () = assert!(std::mem::size_of::<IdList>() == 16);

impl Default for IdList {
    fn default() -> Self {
        IdList::Inline {
            len: 0,
            ids: [0; 3],
        }
    }
}

impl IdList {
    #[inline]
    pub(super) fn as_slice(&self) -> &[u32] {
        match self {
            IdList::Inline { len, ids } => &ids[..*len as usize],
            IdList::Spill(v) => v.ids.as_slice(),
        }
    }

    #[inline]
    pub(super) fn len(&self) -> usize {
        self.as_slice().len()
    }

    #[inline]
    pub(super) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub(super) fn contains(&self, id: u32) -> bool {
        match self {
            IdList::Inline { len, ids } => ids[..*len as usize].contains(&id),
            IdList::Spill(v) => v.position(id).is_some(),
        }
    }

    fn spill(&mut self) -> &mut SpillList {
        if let IdList::Inline { len, ids } = self {
            let v = SpillList {
                ids: ids[..*len as usize].to_vec(),
                pos: crate::fast_hash::new_ptr_hash_map(),
            };
            *self = IdList::Spill(Box::new(v));
        }
        match self {
            IdList::Spill(v) => v,
            IdList::Inline { .. } => unreachable!(),
        }
    }

    /// Append `id` unless already present.
    pub(super) fn push_back(&mut self, id: u32) {
        if self.contains(id) {
            return;
        }
        self.append_unchecked(id);
    }

    /// Append an id the caller knows is not in this list.
    ///
    /// `alloc_shape_id` hands out a strictly increasing counter that is never
    /// reused (it parks at `SHAPE_ID_END` rather than wrapping), so an id that
    /// was allocated after this list was built cannot be in it, in this family
    /// or in any other. The membership scan in [`push_back`] is therefore dead
    /// work at the two interning sites, and it is not O(1) dead work: a family
    /// holds every descriptor ever created for one keys array, so the scan is
    /// linear in the history of that keys array and interning the *n*-th
    /// descriptor for it costs O(n) — quadratic over a render that keeps
    /// bumping a shape's semantic generation. `IdList::contains` was 6.2 % of
    /// main-thread leaf samples on a claude-code streamed reply, 95 % of it
    /// under `ShapeTableInner::family_push_back`.
    ///
    /// The spill index now removes that scan for the callers that cannot use
    /// this entry point, which is why `contains` is O(1) above
    /// [`SPILL_INDEX_MIN`]. This function stays because skipping the probe
    /// entirely is still cheaper than performing it.
    ///
    /// Callers that re-file an EXISTING id (the metadata rekey when a keys
    /// array moves) must keep using [`push_back`]: those ids can already be in
    /// the destination list.
    pub(super) fn append_unchecked(&mut self, id: u32) {
        match self {
            IdList::Inline { len, ids } if (*len as usize) < ids.len() => {
                ids[*len as usize] = id;
                *len += 1;
            }
            _ => self.spill().push(id),
        }
    }

    /// Prepend `id` unless already present. Order-preserving by definition, so
    /// it stays O(n) on a spilled list; only `by_facts` and the external-id
    /// install use it, and neither is on a hot path.
    pub(super) fn push_front(&mut self, id: u32) {
        if self.contains(id) {
            return;
        }
        match self {
            IdList::Inline { len, ids } if (*len as usize) < ids.len() => {
                ids.copy_within(0..*len as usize, 1);
                ids[0] = id;
                *len += 1;
            }
            _ => {
                let v = self.spill();
                v.ids.insert(0, id);
                if v.indexed() {
                    v.pos.clear();
                }
                v.maybe_build_index();
            }
        }
    }

    /// Drop `id` if present, PRESERVING the order of what remains; returns
    /// whether it was there. For a list whose order is load-bearing —
    /// `by_facts`, where the first entry is the canonical answer.
    pub(super) fn remove_ordered(&mut self, id: u32) -> bool {
        match self {
            IdList::Inline { len, ids } => Self::remove_inline(len, ids, id),
            IdList::Spill(v) => v.remove_ordered(id).is_some(),
        }
    }

    /// Drop `id` if present, WITHOUT preserving order; returns whether it was
    /// there. For `families`, whose readers are set-valued (see the type doc).
    ///
    /// This is the change: on a spilled list it moves ONE element instead of
    /// the whole tail.
    pub(super) fn remove_unordered(&mut self, id: u32) -> bool {
        match self {
            // Three entries: the inline shift is a single register move and
            // there is nothing to gain from disturbing the order.
            IdList::Inline { len, ids } => Self::remove_inline(len, ids, id),
            IdList::Spill(v) => v.remove_unordered(id).is_some(),
        }
    }

    #[inline]
    fn remove_inline(len: &mut u8, ids: &mut [u32; 3], id: u32) -> bool {
        let n = *len as usize;
        note_scan(n);
        let Some(pos) = ids[..n].iter().position(|&x| x == id) else {
            return false;
        };
        note_removal(n - 1 - pos);
        ids.copy_within(pos + 1..n, pos);
        ids[n - 1] = 0;
        *len -= 1;
        true
    }

    /// Replace `old` with `new` in place (keeps its position); returns
    /// whether `old` was present.
    pub(super) fn replace(&mut self, old: u32, new: u32) -> bool {
        match self {
            IdList::Inline { len, ids } => {
                let n = *len as usize;
                note_scan(n);
                match ids[..n].iter().position(|&x| x == old) {
                    Some(pos) => {
                        ids[pos] = new;
                        true
                    }
                    None => false,
                }
            }
            IdList::Spill(v) => v.replace(old, new),
        }
    }

    pub(super) fn heap_bytes(&self) -> usize {
        match self {
            IdList::Inline { .. } => 0,
            IdList::Spill(v) => {
                std::mem::size_of::<SpillList>()
                    + v.ids.capacity() * 4
                    // The index is the structure's memory cost and is reported
                    // rather than hidden: it exists only above SPILL_INDEX_MIN.
                    + v.pos.capacity() * (std::mem::size_of::<(u32, u32)>() + 1)
            }
        }
    }
}
