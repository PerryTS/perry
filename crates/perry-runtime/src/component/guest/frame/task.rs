//! Stable C task frames own a native scheduling lease and a traced ALS snapshot.
//! No native handle scope or borrowed managed address survives a C return.
use super::*;
use crate::async_context::{self as als, job_domain};

pub(super) struct TaskContext {
    domain: job_domain::JobDomain,
    snapshot: u32,
}

impl Frame {
    fn task_guard<T: Default>(&mut self, body: impl FnOnce(&mut Self) -> T) -> T {
        if self.failed {
            return T::default();
        }
        let Some(task) = &self.task else {
            return self.guard(|_| fail("Expected a component task frame"));
        };
        if task.domain.is_cancelled() {
            return self.guard(|_| fail("Component task is cancelled"));
        }
        let token = task.domain.token();
        let slot = task.snapshot;
        let snapshot = unsafe {
            (*JSValue::from_bits(self.get(slot).to_bits()).as_pointer::<als::AsyncSnapshotHolder>())
                .snapshot
                .clone()
        };
        job_domain::run(token, || {
            let previous = als::enter_context(&snapshot);
            drop(snapshot);
            als::push_context_guard(als::ContextGuardAction::RestoreSnapshot(previous));
            let value = self.guard(body);
            // Snapshot allocation suppresses collection until native contents
            // are installed. The new holder is then rooted before array growth.
            let saved = als::alloc_snapshot_holder(als::capture_context());
            let saved = crate::value::js_nanbox_pointer(saved as i64);
            crate::array::js_array_set_f64(array(self.values()), slot, saved);
            if let Some(action) = als::pop_context_guard() {
                als::apply_context_guard(action);
            }
            value
        })
    }
}

#[no_mangle]
pub extern "C" fn perry_component_frame_task_new() -> *mut c_void {
    let mut frame = Box::new(Frame::allocate_guest_frame());
    let saved = als::alloc_snapshot_holder(als::capture_context());
    let snapshot = frame.add(crate::value::js_nanbox_pointer(saved as i64));
    frame.task = Some(TaskContext {
        domain: job_domain::JobDomain::new(),
        snapshot,
    });
    Box::into_raw(frame).cast()
}

/// Call without a global checkpoint. The C scheduler checkpoints this task
/// separately, then restores every Rust scope before canonical suspension.
/// # Safety
/// `pointer` is an open task frame on its creating agent; slots and immutable
/// export indices belong to that frame and its generated contract.
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_task_call(
    pointer: *mut c_void,
    interface: u32,
    function: u32,
    args: u32,
) -> u32 {
    frame(pointer).task_guard(|frame| call_export(frame, interface, function, args))
}

/// Drain this task's ready Promise, nextTick and cleanup jobs. Timers and
/// foreign native I/O are not driven. Returns -1 with the original frame error.
/// # Safety
/// `pointer` is an open task frame on its creating agent, exclusively borrowed.
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_task_checkpoint(pointer: *mut c_void) -> i32 {
    let owner = frame(pointer);
    let count = owner.task_guard(|_| crate::promise::microtasks::guest_task_checkpoint());
    if owner.failed {
        -1
    } else {
        count
    }
}

/// Consume a native wake hint and check runnable work: 1 ready, 0 no work,
/// -1 frame error. This also notices unowned jobs queued outside any task.
/// Readiness does not replace a canonical waitable or establish task EXIT.
/// # Safety
/// `pointer` is an open task frame on its creating agent, exclusively borrowed.
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_task_ready(pointer: *mut c_void) -> i32 {
    let owner = frame(pointer);
    let ready = owner.guard(|owner| {
        let task = owner
            .task
            .as_ref()
            .unwrap_or_else(|| fail("Expected a component task frame"));
        let hint = task.domain.take_ready();
        if task.domain.is_cancelled() {
            fail("Component task is cancelled");
        }
        let queued = job_domain::run(
            task.domain.token(),
            crate::promise::eligible_guest_jobs_pending,
        );
        i32::from(hint || queued)
    });
    if owner.failed {
        -1
    } else {
        ready
    }
}

/// Stop this task's queued JS jobs before canonical child cancellation. This
/// does not free the task/import frames or acknowledge native child storage;
/// C retains them until each child has returned or acknowledged cancellation.
/// Repeated cancellation is harmless. No JS callback is invoked.
/// # Safety
/// `pointer` is an open task frame on its creating agent, exclusively borrowed;
/// no task call/checkpoint/resume may still be active on the C stack.
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_task_cancel(pointer: *mut c_void) -> i32 {
    let owner = frame(pointer);
    owner.guard(|owner| {
        let task = owner
            .task
            .as_ref()
            .unwrap_or_else(|| fail("Expected a component task frame"));
        task.domain.cancel();
        crate::promise::discard_cancelled_guest_jobs();
    });
    if owner.failed {
        -1
    } else {
        0
    }
}

/// Execute a finite C completion callback under this task's saved context.
/// The callback may lift values into a DISTINCT import frame and settle its
/// Promise. It must return before the canonical scheduler chooses WAIT/EXIT.
/// # Safety
/// `pointer` is an open task frame on its creating agent. A non-null callback
/// and `data` are valid for this synchronous invocation. The callback must not
/// reenter any frame API with `pointer`, drop it, or canonically suspend.
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_task_resume(
    pointer: *mut c_void,
    callback: Option<unsafe extern "C" fn(*mut c_void)>,
    data: *mut c_void,
) -> i32 {
    let owner = frame(pointer);
    owner.task_guard(|_| {
        let callback = callback.unwrap_or_else(|| fail("Missing component task callback"));
        callback(data);
    });
    if owner.failed {
        -1
    } else {
        0
    }
}

#[cfg(test)]
mod cancellation_tests;
#[cfg(test)]
mod tests;
