//! The optional engine is registered by the link installer. All calls use its
//! versioned table, so ordinary executables do not link Wasmtime.
use perry_abi::component_host::{ComponentHostOps, ABI_VERSION};
use std::sync::atomic::{AtomicPtr, Ordering};

static OPS: AtomicPtr<ComponentHostOps> = AtomicPtr::new(std::ptr::null_mut());

/// Installs a test table into the process-global `OPS` and holds
/// `TEST_OPS_LOCK` until dropped: two tests with different tables would
/// otherwise swap and restore each other's pointer, and a test whose `ops()`
/// read sees null or a foreign table throws uncaught (the process exits and
/// the thread-local destructors abort the whole lib-test binary).
#[cfg(test)]
pub(super) struct TestOpsGuard {
    previous: *mut ComponentHostOps,
    _serial: std::sync::MutexGuard<'static, ()>,
}
#[cfg(test)]
static TEST_OPS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
#[cfg(test)]
impl TestOpsGuard {
    pub fn install(ops: &'static ComponentHostOps) -> Self {
        let serial = TEST_OPS_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Self {
            previous: OPS.swap(
                (ops as *const ComponentHostOps).cast_mut(),
                Ordering::AcqRel,
            ),
            _serial: serial,
        }
    }
}
#[cfg(test)]
impl Drop for TestOpsGuard {
    fn drop(&mut self) {
        OPS.store(self.previous, Ordering::Release);
    }
}

/// # Safety
/// A non-null table must be immutable and remain valid for the process lifetime.
/// Its entries are code addresses, never Perry-managed heap values. The native
/// engine installer passes its image's static `ffi::OPS` table.
#[no_mangle]
pub unsafe extern "C" fn js_runtime_set_component_host_ops(ops: *const ComponentHostOps) {
    if !ops.is_null()
        && (*ops).abi_version == ABI_VERSION
        && (*ops).size == std::mem::size_of::<ComponentHostOps>()
    {
        OPS.store(ops.cast_mut(), Ordering::Release);
    }
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_REGISTER: unsafe extern "C" fn(*const ComponentHostOps) =
    js_runtime_set_component_host_ops;

pub(super) fn ops() -> &'static ComponentHostOps {
    let ops = OPS.load(Ordering::Acquire);
    if ops.is_null() {
        super::fail("Component host is missing or has an incompatible ABI; install matching Perry component-host and runtime archives");
    }
    unsafe { &*ops }
}
