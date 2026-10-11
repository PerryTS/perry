//! The record-owned inherited [[Set]] fact extension. No GC pointers.

use super::{new_extras, ShapeExtras, ShapeRecord};

#[derive(Debug)]
pub(crate) struct StoreChainFact {
    pub(crate) key: Box<[u8]>,
    pub(crate) validity: u64,
    pub(crate) blocked: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unused_extensions_keep_store_facts_absent_and_preserve_identity() {
        let mut record = ShapeRecord::EMPTY;
        record.extras = new_extras(&[], &[17]);
        let original = record.extras;
        assert!(record.store_chain_facts().is_none());
        assert!(record.ensure_store_chain_facts().borrow().is_empty());
        assert_eq!(
            record.extras, original,
            "the identity extension must be retained"
        );
        let extras = unsafe { &*(record.extras as usize as *const ShapeExtras) };
        assert_eq!(&*extras.brands, &[17]);
        // This standalone record was never published into the shape slab.
        unsafe { drop(Box::from_raw(record.extras as usize as *mut ShapeExtras)) };
    }
}
impl ShapeRecord {
    /// Native allocations owned by this record, observed only by the heap
    /// census. These are outside the managed heap's live-byte count.
    pub(crate) fn extension_census(&self) -> (usize, usize, usize, usize) {
        if self.extras == 0 || self.extras & 1 != 0 {
            return (0, 0, 0, 0);
        }
        let extras = unsafe { &*(self.extras as usize as *const ShapeExtras) };
        let mut bytes = std::mem::size_of::<ShapeExtras>()
            + std::mem::size_of_val(extras.constfn_infos.as_ref())
            + std::mem::size_of_val(extras.brands.as_ref());
        let (facts, fact_bytes) = extras.store_chain.as_ref().map_or((0, 0), |chain| {
            let facts = chain.borrow();
            (
                facts.len(),
                std::mem::size_of_val(chain.as_ref())
                    + facts.capacity() * std::mem::size_of::<StoreChainFact>()
                    + facts.iter().map(|fact| fact.key.len()).sum::<usize>(),
            )
        });
        bytes += fact_bytes;
        (1, bytes, facts, fact_bytes)
    }

    pub(crate) fn store_chain_facts(&self) -> Option<&std::cell::RefCell<Vec<StoreChainFact>>> {
        if self.extras == 0 || self.extras & 1 != 0 {
            None
        } else {
            // The slab record owns this extension for its entire lifetime.
            unsafe {
                (*(self.extras as usize as *const ShapeExtras))
                    .store_chain
                    .as_deref()
            }
        }
    }

    pub(crate) fn ensure_store_chain_facts(&mut self) -> &std::cell::RefCell<Vec<StoreChainFact>> {
        if self.extras == 0 || self.extras & 1 != 0 {
            let parent = self.rollback_parent();
            self.extras = new_extras(&[], &[]);
            self.note_rollback_parent(parent);
        }
        let extras = unsafe { &mut *(self.extras as usize as *mut ShapeExtras) };
        extras
            .store_chain
            .get_or_insert_with(|| Box::new(std::cell::RefCell::new(Vec::new())))
    }
}
