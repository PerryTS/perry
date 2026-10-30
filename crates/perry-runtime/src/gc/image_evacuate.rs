//! Evacuating the realm image into this agent's heap (DESIGN §2.5, owner
//! decision §7.1): the mechanism a worker agent's realm is born by.
//!
//! Realm objects and function cells are copied into an ordinary heap cell of its type and size,
//! then the layout tracer visits each copy's traced slots and rewrites every
//! slot that names an image cell to the same offset in that cell's copy.
//! Pointers to image strings, `JsFunctionInfo` statics and atoms lie outside
//! the range and stay. No relocation list exists: the layouts are the
//! relocation information, so a pointer no layout describes would survive
//! the evacuation and be shared between agents. [`words_pointing_into_image`]
//! is the check for that (DESIGN §6 risk 3).
use super::*;

/// The copies one evacuation made, in image address order:
/// `(image header, size, copy header)`. The caller keeps it only to find the
/// copies of its roots (the global object); nothing retains it.
pub(crate) struct RealmEvacuation {
    cells: Vec<(usize, usize, usize)>,
}

impl RealmEvacuation {
    /// The forwarding rule: an address inside an image cell moves to the same
    /// offset inside that cell's copy. `None` outside every image cell.
    pub(crate) fn forward(&self, addr: usize) -> Option<usize> {
        let index = self
            .cells
            .partition_point(|&(header, _, _)| header <= addr)
            .checked_sub(1)?;
        let (header, size, copy) = self.cells[index];
        (addr < header + size).then(|| copy + (addr - header))
    }

    /// The copy headers, in image address order.
    pub(crate) fn copies(&self) -> impl Iterator<Item = *mut GcHeader> + '_ {
        self.cells.iter().map(|&(_, _, copy)| copy as *mut GcHeader)
    }

    pub(crate) fn cell_count(&self) -> usize {
        self.cells.len()
    }
}

/// A word naming an image address, forwarded with its tag kept: a tagged
/// pointer (object, string, bigint) or a raw pointer word.
fn forward_bits(evacuation: &RealmEvacuation, bits: u64) -> Option<u64> {
    #[cfg(test)]
    if super::image_cells::sabotage::knocked(super::image_cells::sabotage::FORWARD) {
        return None;
    }
    let tag = bits & TAG_MASK;
    let (addr, tag) = if bits >> 48 == 0 {
        (bits as usize, 0)
    } else if tag == POINTER_TAG || tag == STRING_TAG || tag == BIGINT_TAG {
        ((bits & POINTER_MASK) as usize, tag)
    } else {
        return None;
    };
    evacuation
        .forward(addr)
        .map(|new_addr| tag | new_addr as u64)
}

/// Copy every image cell into this agent's heap and forward the copies' slots.
/// No collection runs in between: the copies are unreachable until the caller
/// roots what it needs.
pub(crate) fn evacuate_realm_image() -> RealmEvacuation {
    let _no_collection = GcSuppressScope::new();
    let mut cells = Vec::new();
    crate::value::addr_class::realm::for_each_cell(&mut |header| unsafe {
        // Canonical lists are immutable facts shared by all agents. Their
        // addresses, including prefix/interior addresses, never forward.
        if (*header).obj_type == GC_TYPE_ARRAY && (*header).gc_flags & GC_FLAG_SHAPE_SHARED != 0 {
            return;
        }
        let size = (*header).size as usize;
        let payload = size - GC_HEADER_SIZE;
        let user = crate::arena::arena_alloc_gc(payload, 8, (*header).obj_type);
        let copy = header_from_user_ptr(user);
        // Kind facts live in `_reserved` (an object's [[Call]] bit); the
        // allocator owns the flags and size.
        (*copy)._reserved = (*header)._reserved;
        // GC_STORE_AUDIT(INIT): an unpublished copy, filled byte for byte; its
        // pointer slots are forwarded and barriered below.
        std::ptr::copy_nonoverlapping((header as *const u8).add(GC_HEADER_SIZE), user, payload);
        if crate::gc_type_is_object!((*copy).obj_type) && !crate::arena::pointer_in_nursery(user as usize) {
            // What an old-generation birth of this layout records.
            let word = crate::codegen_abi::LayoutWord(
                (*(user as *const crate::object::ObjectHeader)).parent_class_id,
            );
            crate::object::shapes::note_old_generation_carrier(
                crate::object::shapes::shape_record_for_word(word),
            );
        }
        cells.push((header as usize, size, copy as usize));
    });
    let evacuation = RealmEvacuation { cells };
    for copy in evacuation.copies() {
        let user = unsafe { (copy as *mut u8).add(GC_HEADER_SIZE) } as usize;
        unsafe {
            visit_gc_rewrite_slots::<{ cfg!(test) }>(copy, |slot| {
                let Some(new_bits) = forward_bits(&evacuation, slot.read()) else {
                    return;
                };
                slot.write(new_bits);
                runtime_write_barrier_gc_slot(user, slot.slot as usize, new_bits);
            });
        }
    }
    evacuation
}

/// Every word of every copy, described by a layout or not, that still names an
/// address in the image range: `(word address, bits)`. Empty after a correct
/// evacuation; a non-empty answer is a pointer the layouts do not describe.
pub(crate) fn words_pointing_into_image(evacuation: &RealmEvacuation) -> Vec<(usize, u64)> {
    let (start, end) = crate::value::addr_class::realm::bounds();
    let mut found = Vec::new();
    for copy in evacuation.copies() {
        let size = unsafe { (*copy).size } as usize;
        let mut at = copy as usize + GC_HEADER_SIZE;
        while at + 8 <= copy as usize + size {
            let bits = unsafe { std::ptr::read(at as *const u64) };
            let addr = if bits >> 48 == 0 {
                bits as usize
            } else {
                (bits & POINTER_MASK) as usize
            };
            if (start..end).contains(&addr)
                && !unsafe {
                    crate::value::addr_class::realm::header(addr).is_some_and(|h| {
                        h.as_ref().obj_type == GC_TYPE_ARRAY
                            && h.as_ref().gc_flags & GC_FLAG_SHAPE_SHARED != 0
                    })
                }
            {
                found.push((at, bits));
            }
            at += 8;
        }
    }
    found
}
