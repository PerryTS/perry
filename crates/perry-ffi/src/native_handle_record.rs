//! A native owner stores its property bags in its record: one bag per mutator
//! that published one. A bag holds only that mutator's JavaScript heap
//! references, so only that mutator reads, writes or scans it. Another thread
//! that sets a property on the same handle gets its own bag, as it did when the
//! bags lived in per-thread tables.
use std::any::Any;
use std::thread::ThreadId;

/// Property storage belonging to one native owner, one bag per mutator.
#[derive(Default)]
pub struct NativePropertyBag {
    bags: Vec<(ThreadId, u64)>,
}
impl NativePropertyBag {
    /// The current mutator's bag bits; zero when it has not published a bag.
    pub fn get(&self) -> Option<u64> {
        let me = std::thread::current().id();
        Some(
            self.bags
                .iter()
                .find(|(owner, _)| *owner == me)
                .map_or(0, |&(_, bits)| bits),
        )
    }
    /// Publish (or, with zero, drop) the current mutator's bag bits.
    pub fn set(&mut self, bits: u64) -> bool {
        let me = std::thread::current().id();
        match self.bags.iter().position(|(owner, _)| *owner == me) {
            Some(index) if bits == 0 => {
                self.bags.swap_remove(index);
            }
            Some(index) => self.bags[index].1 = bits,
            None if bits != 0 => self.bags.push((me, bits)),
            None => {}
        }
        true
    }
    /// Return this mutator's nonempty bag slot for GC tracing and rewriting.
    pub fn slot(&mut self) -> Option<&mut u64> {
        let me = std::thread::current().id();
        self.bags
            .iter_mut()
            .find(|(owner, bits)| *owner == me && *bits != 0)
            .map(|(_, bits)| bits)
    }
    /// Allocation ranges remain usable after the mutator's TLS has retired.
    pub fn clear_if(&mut self, mut freed: impl FnMut(usize) -> bool) {
        self.bags
            .retain(|&(_, bits)| !freed((bits & 0x0000_ffff_ffff_ffff) as usize));
    }
}
/// A registered native payload with its own JavaScript property storage.
pub struct NativePayloadRecord {
    /// The typed native payload; removal preserves its existing downcast API.
    pub payload: Box<dyn Any + Send + Sync>,
    props: NativePropertyBag,
}
impl NativePayloadRecord {
    /// Wrap a native payload with an initially empty property bag.
    pub fn new<T: Any + Send + Sync>(payload: T) -> Self {
        Self {
            payload: Box::new(payload),
            props: Default::default(),
        }
    }
    /// Read the current mutator's property bag bits.
    pub fn props(&self) -> Option<u64> {
        self.props.get()
    }
    /// Replace this mutator's bag bits.
    pub fn set_props(&mut self, bits: u64) -> bool {
        self.props.set(bits)
    }
    /// Return this mutator's nonempty bag slot for the existing owner scanner.
    pub fn props_slot(&mut self) -> Option<&mut u64> {
        self.props.slot()
    }
    /// Clear the bags whose allocations belong to the retiring mutator.
    pub fn clear_props_if(&mut self, freed: impl FnMut(usize) -> bool) {
        self.props.clear_if(freed);
    }
    /// Borrow the underlying typed native payload.
    pub fn as_mut(&mut self) -> &mut (dyn Any + Send + Sync) {
        self.payload.as_mut()
    }
}
impl std::ops::Deref for NativePayloadRecord {
    type Target = dyn Any + Send + Sync;
    fn deref(&self) -> &Self::Target {
        self.payload.as_ref()
    }
}
impl std::ops::DerefMut for NativePayloadRecord {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.payload.as_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each mutator owns its own bag on a shared native owner: a second
    /// thread can publish a property bag without disturbing the first, and
    /// neither sees or scans the other's heap references.
    #[test]
    fn each_mutator_publishes_its_own_bag() {
        let mut bag = NativePropertyBag::default();
        assert_eq!(bag.get(), Some(0));
        assert!(bag.set(0x1000));
        assert_eq!(bag.get(), Some(0x1000));
        let bag = std::sync::Mutex::new(bag);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                let mut bag = bag.lock().unwrap();
                assert_eq!(bag.get(), Some(0), "another mutator starts without a bag");
                assert!(bag.slot().is_none(), "nor may it scan the first one's");
                assert!(bag.set(0x2000), "another mutator may publish its own bag");
                assert_eq!(bag.get(), Some(0x2000));
                assert_eq!(bag.slot().map(|slot| *slot), Some(0x2000));
            });
        });
        let mut bag = bag.into_inner().unwrap();
        assert_eq!(bag.get(), Some(0x1000));
        assert_eq!(bag.slot().map(|slot| *slot), Some(0x1000));
        // The second mutator's arena retires: only its bag goes.
        bag.clear_if(|address| address == 0x2000);
        assert_eq!(bag.bags.len(), 1);
        assert!(bag.set(0));
        assert!(bag.bags.is_empty());
    }
}
