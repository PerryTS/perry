//! Direct canonical ABI wait-set. No executor or task-local registration map.
//! Each waitable is owned by a generational request or the current deadline.
#[link(wasm_import_module = "$root")]
unsafe extern "C" {
    #[link_name = "[waitable-set-new]"]
    fn set_new() -> u32;
    #[link_name = "[waitable-set-drop]"]
    fn set_drop(set: u32);
    #[link_name = "[waitable-join]"]
    fn raw_join(waitable: u32, set: u32);
    #[link_name = "[waitable-set-wait]"]
    fn wait(set: u32, payload: *mut [u32; 2]) -> u32;
    #[link_name = "[waitable-set-poll]"]
    fn poll(set: u32, payload: *mut [u32; 2]) -> u32;
    #[link_name = "[subtask-cancel]"]
    pub(super) fn raw_subtask_cancel(task: u32) -> u32;
    #[link_name = "[subtask-drop]"]
    pub(super) fn raw_subtask_drop(task: u32);
}
pub struct WaitSet(u32);
impl WaitSet {
    pub fn new() -> Self {
        // SAFETY: intrinsic returns a new owned wait-set.
        Self(unsafe { set_new() })
    }
    pub fn join(&self, waitable: u32) {
        // SAFETY: caller owns the live waitable until removing its membership.
        unsafe {
            super::abi::preserving_stack(|| raw_join(waitable, self.0));
        }
    }
    pub fn remove(&self, waitable: u32) {
        // SAFETY: caller still owns this waitable; zero removes all membership.
        unsafe {
            super::abi::preserving_stack(|| raw_join(waitable, 0));
        }
    }
    pub fn step(&self, blocking: bool) -> (u32, u32, u32) {
        let mut payload = [0; 2];
        // SAFETY: live wait-set and writable aligned event payload.
        let kind = super::abi::preserving_stack(|| unsafe {
            if blocking {
                wait(self.0, &mut payload)
            } else {
                poll(self.0, &mut payload)
            }
        });
        (kind, payload[0], payload[1])
    }
}
impl Drop for WaitSet {
    fn drop(&mut self) {
        // SAFETY: backend cancels all outstanding operations before dropping its set.
        unsafe {
            set_drop(self.0);
        }
    }
}

pub(super) unsafe fn subtask_cancel(task: u32) -> u32 {
    // SAFETY: caller owns the live task and its pinned return area until cancellation.
    super::abi::preserving_stack(|| unsafe { raw_subtask_cancel(task) })
}
pub(super) unsafe fn subtask_drop(task: u32) {
    // SAFETY: caller removed membership and observed return/cancellation.
    super::abi::preserving_stack(|| unsafe { raw_subtask_drop(task) })
}
