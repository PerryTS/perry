//! Poll-local, origin-indexed frames. Native callbacks may add another origin
//! while an earlier frame is executing. Boxes stabilize C tokens; ordinary
//! array edges keep late-created roots alive past transient callback scopes.
use super::*;
use crate::{async_context as als, async_hooks::AsyncResourceIds};
use std::{cell::RefCell, collections::BTreeMap, ops::Deref};

pub(in crate::component) struct FrameBatch<'a> {
    owner: RuntimeHandle<'a>,
    rows: RuntimeHandle<'a>,
    frames: RefCell<BTreeMap<u64, Box<InvocationFrame<'a>>>>,
}

impl<'a> FrameBatch<'a> {
    /// Construct before entering the native poll. These two handle slots must
    /// outlive every transient callback/exception scope created during it.
    pub fn new(scope: &'a RuntimeHandleScope, owner: f64) -> Self {
        let owner = scope.root_nanbox_f64(owner);
        let rows = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::array::js_array_alloc(0) as i64,
        ));
        Self {
            owner,
            rows,
            frames: RefCell::new(BTreeMap::new()),
        }
    }

    /// Saved records are ordinary component-owned edges between polls. No
    /// public Promise or previous poll frame is needed to create this frame.
    pub fn for_context(
        &self,
        origin: u64,
        record: f64,
        ids: AsyncResourceIds,
    ) -> &InvocationFrame<'a> {
        let existing = self
            .frames
            .borrow()
            .get(&origin)
            .map(|frame| &**frame as *const InvocationFrame<'a>);
        let pointer = existing.unwrap_or_else(|| {
            let frame = Box::new(InvocationFrame::new(
                self.rows,
                self.owner.get_nanbox_f64(),
                record,
                ids,
            ));
            let mut frames = self.frames.borrow_mut();
            &**frames.entry(origin).or_insert(frame) as *const InvocationFrame<'a>
        });
        // Boxes are never replaced or removed, and cannot outlive this batch.
        unsafe { &*pointer }
    }

    #[cfg(test)]
    fn for_promise(&self, origin: u64, promise: f64) -> &InvocationFrame<'a> {
        if self.frames.borrow().contains_key(&origin) {
            return self.for_context(
                origin,
                undefined(),
                AsyncResourceIds {
                    async_id: 0,
                    trigger_async_id: 0,
                },
            );
        }
        let (record, ids) = super::saved::capture(promise);
        self.for_context(origin, record, ids)
    }
}

/// A slot in a private row of the batch's traced array. The only native roots
/// are the two handles allocated before the poll; no transient handle index or
/// copied JS address survives a callback.
#[derive(Clone, Copy)]
pub(in crate::component) struct RowSlot<'a> {
    rows: RuntimeHandle<'a>,
    row: u32,
    column: u32,
}
impl RowSlot<'_> {
    pub fn get_nanbox_f64(&self) -> f64 {
        let row = crate::array::js_array_get_f64(
            array_pointer(self.rows.get_nanbox_f64()).unwrap(),
            self.row,
        );
        crate::array::js_array_get_f64(array_pointer(row).unwrap(), self.column)
    }
    pub fn set_nanbox_f64(&self, value: f64) {
        let row = crate::array::js_array_get_f64(
            array_pointer(self.rows.get_nanbox_f64()).unwrap(),
            self.row,
        );
        crate::array::js_array_set_f64(array_pointer(row).unwrap(), self.column, value);
    }
}

pub(in crate::component) struct InvocationFrame<'a> {
    frame: Frame<'a>,
    snapshot: RowSlot<'a>,
    metadata: RowSlot<'a>,
    serial: u64,
    trigger: u64,
}

impl<'a> Deref for InvocationFrame<'a> {
    type Target = Frame<'a>;
    fn deref(&self) -> &Self::Target {
        &self.frame
    }
}

impl<'a> InvocationFrame<'a> {
    fn new(rows: RuntimeHandle<'a>, owner: f64, record: f64, ids: AsyncResourceIds) -> Self {
        let scope = RuntimeHandleScope::new();
        let owner = scope.root_nanbox_f64(owner);
        let record = scope.root_nanbox_f64(record);
        let record = array_pointer(record.get_nanbox_f64()).expect("saved invocation context");
        let snapshot = scope.root_nanbox_f64(crate::array::js_array_get_f64(record, 0));
        let metadata = scope.root_nanbox_f64(crate::array::js_array_get_f64(record, 1));
        let row = scope.root_raw_mut_ptr(crate::array::js_array_alloc_with_length(4));
        for (index, value) in [
            owner.get_nanbox_f64(),
            undefined(),
            snapshot.get_nanbox_f64(),
            metadata.get_nanbox_f64(),
        ]
        .into_iter()
        .enumerate()
        {
            crate::array::js_array_set_f64(row.get_raw_mut_ptr(), index as u32, value);
        }
        let index = crate::array::js_array_length(array_pointer(rows.get_nanbox_f64()).unwrap());
        let grown = crate::array::js_array_set_f64_extend(
            array_pointer(rows.get_nanbox_f64()).unwrap(),
            index,
            crate::value::js_nanbox_pointer(
                row.get_raw_mut_ptr::<crate::array::ArrayHeader>() as i64
            ),
        );
        rows.set_nanbox_f64(crate::value::js_nanbox_pointer(grown as i64));
        let slot = |column| RowSlot {
            rows,
            row: index,
            column,
        };
        Self {
            frame: Frame {
                owner: FrameValue::Row(slot(0)),
                error: FrameValue::Row(slot(1)),
                threw: Cell::new(false),
            },
            snapshot: slot(2),
            metadata: slot(3),
            serial: ids.async_id,
            trigger: ids.trigger_async_id,
        }
    }

    fn context(&self) -> als::AsyncContextSnapshot {
        unsafe {
            (*JSValue::from_bits(self.snapshot.get_nanbox_f64().to_bits())
                .as_pointer::<als::AsyncSnapshotHolder>())
            .snapshot
            .clone()
        }
    }

    pub fn run(&self, completion: impl FnOnce() -> f64) -> Result<f64, f64> {
        let metadata = JSValue::from_bits(self.metadata.get_nanbox_f64().to_bits());
        let ids = AsyncResourceIds {
            async_id: if metadata.is_undefined() {
                self.serial
            } else {
                metadata.as_pointer::<u8>() as usize as u64
            },
            trigger_async_id: self.trigger,
        };
        crate::async_hooks::try_run_captured_promise_scope(ids, self.context(), completion)
    }

    /// Retirement must still execute when a before hook throws. Restore the
    /// saved ALS context without calling that hook again, then restore the
    /// caller context even when a destructor throws.
    pub fn run_cleanup(&self, completion: impl FnOnce() -> f64) -> Result<f64, f64> {
        let scope = RuntimeHandleScope::new();
        let previous = scope.root_raw_mut_ptr(als::alloc_snapshot_holder(als::capture_context()));
        drop(als::enter_context(&self.context()));
        let outcome = crate::exception::catch_js_throw(completion);
        let value = scope.root_nanbox_f64(outcome.unwrap_or_else(|error| error));
        unsafe {
            als::restore_context(
                (*previous.get_raw_mut_ptr::<als::AsyncSnapshotHolder>())
                    .snapshot
                    .clone(),
            );
        }
        if outcome.is_err() {
            Err(value.get_nanbox_f64())
        } else {
            Ok(value.get_nanbox_f64())
        }
    }
}

#[cfg(test)]
mod root0_tests;
#[cfg(test)]
mod tests;
