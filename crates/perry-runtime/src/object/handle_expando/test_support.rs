//! A real C-frame-owned native bag slot for runtime-only adapter fixtures.
use std::cell::UnsafeCell;
const MAGIC: u64 = 0xface_0000_1221_ba69;
#[repr(C)]
struct Record {
    magic: u64,
    bag: UnsafeCell<f64>,
}
/// The 8 bytes before a native owner are whatever the allocator keeps there,
/// and the descriptor LOOKUP path (`attrs_live_in_keys`, which must prove
/// allocator ownership before interpreting the preceding bytes)
/// decodes them as a `GcHeader`: stale bytes that read as an unforwarded
/// OBJECT header send the lookup into the wrong memory. The guard word pins
/// those 8 bytes to zero so the fixture does not depend on heap layout.
#[repr(C)]
struct Guarded {
    guard: u64,
    record: Record,
}
thread_local! {
    /// The records of this thread's live owners: a handle the bag dispatch is
    /// asked about is dereferenced only when it is one of them (a typed handle
    /// dispatcher probes the bag of every handle it sees, own properties first).
    static OWNER_RECORDS: std::cell::RefCell<Vec<usize>> =
        const { std::cell::RefCell::new(Vec::new()) };
}
pub(crate) struct TestNativeOwner {
    // Drop the root scope before freeing the native argument cell.
    _roots: crate::gc::RuntimeHandleScope,
    owner: Box<Guarded>,
}
impl TestNativeOwner {
    pub(crate) fn new() -> Self {
        unsafe {
            super::super::class_handles::js_register_native_property_bag_dispatch(access);
        }
        let owner = Box::new(Guarded {
            guard: 0,
            record: Record {
                magic: MAGIC,
                bag: UnsafeCell::new(0.0),
            },
        });
        let roots = crate::gc::RuntimeHandleScope::new();
        unsafe {
            roots.root_nanbox_cell(&owner.record.bag);
        }
        let record = &owner.record as *const Record as usize;
        OWNER_RECORDS.with(|o| o.borrow_mut().push(record));
        Self {
            _roots: roots,
            owner,
        }
    }
    pub(crate) fn handle(&self) -> i64 {
        (&self.owner.record as *const Record) as i64
    }
    pub(crate) fn clear(&self) {
        unsafe {
            *self.owner.record.bag.get() = 0.0;
        }
    }
}
impl Drop for TestNativeOwner {
    fn drop(&mut self) {
        let record = &self.owner.record as *const Record as usize;
        OWNER_RECORDS.with(|o| o.borrow_mut().retain(|r| *r != record));
    }
}
thread_local! {
    static SMALL_HANDLES: std::cell::RefCell<Vec<(i64, *const Record)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}
/// A small handle-band id (the node:http IncomingMessage shape) whose bag
/// lives in a real native record, as the stdlib handle registry supplies it.
pub(crate) struct TestSmallHandleOwner {
    id: i64,
    _owner: TestNativeOwner,
}
impl TestSmallHandleOwner {
    pub(crate) fn new(id: i64) -> Self {
        assert!(crate::value::addr_class::is_handle_band(id as usize));
        let owner = TestNativeOwner::new();
        let record = &owner.owner.record as *const Record;
        SMALL_HANDLES.with(|s| s.borrow_mut().push((id, record)));
        Self { id, _owner: owner }
    }
    /// The handle id NaN-boxed under HANDLE_TAG.
    pub(crate) fn value(&self) -> f64 {
        f64::from_bits(crate::value::HANDLE_TAG | self.id as u64)
    }
}
impl Drop for TestSmallHandleOwner {
    fn drop(&mut self) {
        SMALL_HANDLES.with(|s| s.borrow_mut().retain(|(id, _)| *id != self.id));
    }
}
unsafe extern "C" fn access(handle: i64, operation: u32, bits: u64, out: *mut u64) -> bool {
    if handle <= 0 || out.is_null() {
        return false;
    }
    let record = if crate::value::addr_class::is_handle_band(handle as usize) {
        match SMALL_HANDLES.with(|s| s.borrow().iter().find(|(id, _)| *id == handle).map(|e| e.1)) {
            Some(record) => record as *mut Record,
            None => return false,
        }
    } else if OWNER_RECORDS.with(|o| o.borrow().contains(&(handle as usize))) {
        handle as *mut Record
    } else {
        return false;
    };
    if (*record).magic != MAGIC {
        return false;
    }
    *out = (*(*record).bag.get()).to_bits();
    match operation {
        0 => true,
        1 => {
            *(*record).bag.get() = f64::from_bits(bits);
            true
        }
        _ => false,
    }
}
