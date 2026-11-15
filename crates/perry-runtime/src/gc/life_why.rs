//! LIFE I3d (`PERRY_LIFE_REGIONS_WHY=<path>`, the instrumented runtime only):
//! the parent record of a synchronous full's mark, so the region measurement
//! can name the chain that holds a surviving inside object back to the root
//! that started it. Armed only around the region probe's own full; every mark
//! site of the full trace notes the object that set the bit (the traced
//! parent, or no parent and the root-scan subphase plus the native frame slot
//! being walked).
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

static ARMED: AtomicBool = AtomicBool::new(false);

/// Why a header was marked: the parent header (0: a root), the root-scan
/// subphase, and for a native stack-map root the function address and slot.
#[derive(Clone, Copy)]
pub(crate) struct Why {
    pub(crate) parent: usize,
    pub(crate) phase: &'static str,
    pub(crate) func: usize,
    pub(crate) slot: usize,
}

crate::perry_thread_local! {
    static PARENT: Cell<usize> = const { Cell::new(0) };
    static PHASE: Cell<&'static str> = const { Cell::new("") };
    static ROOT_FN: Cell<usize> = const { Cell::new(0) };
    static MAP: RefCell<HashMap<usize, Why>> = RefCell::new(HashMap::new());
}

#[inline(always)]
pub(crate) fn armed() -> bool {
    ARMED.load(Ordering::Relaxed)
}

/// Arms the record for the next full (clearing the previous one) or disarms it.
pub(crate) fn arm(on: bool) {
    if on {
        MAP.with(|m| m.borrow_mut().clear());
        PARENT.with(|p| p.set(0));
        PHASE.with(|p| p.set("pre_root"));
    }
    ARMED.store(on, Ordering::Relaxed);
}

pub(super) fn phase(name: &'static str) {
    if armed() {
        PHASE.with(|p| p.set(name));
        ROOT_FN.with(|p| p.set(0));
    }
}

/// The registered root scanner being run (its function address).
pub(super) fn root_fn(f: usize) {
    if armed() {
        ROOT_FN.with(|p| p.set(f));
    }
}

pub(super) fn parent(header: usize) {
    if armed() {
        PARENT.with(|p| p.set(header));
    }
}

/// `header` was just marked by the full trace.
pub(super) fn marked(header: usize) {
    if !armed() {
        return;
    }
    let parent = PARENT.with(|p| p.get());
    let (func, slot) = if parent == 0 {
        super::pin::native_root_slot_context()
            .map(|c| (c.function_address, c.slot_addr))
            .unwrap_or((ROOT_FN.with(|p| p.get()), 0))
    } else {
        (0, 0)
    };
    let why = Why {
        parent,
        phase: PHASE.with(|p| p.get()),
        func,
        slot,
    };
    MAP.with(|m| {
        m.borrow_mut().entry(header).or_insert(why);
    });
}

pub(crate) fn get(header: usize) -> Option<Why> {
    MAP.with(|m| m.borrow().get(&header).copied())
}

/// One-line label of the object at `header`: type, size, and for a string
/// its first characters, for an object its class name.
pub(crate) fn label(header: usize) -> String {
    unsafe { super::heap_snapshot::life_label(header as *mut super::GcHeader) }
}

/// Symbol of a code address (`dladdr`), or the address.
#[cfg(unix)]
pub(crate) fn symbol(addr: usize) -> String {
    let mut info: libc::Dl_info = unsafe { std::mem::zeroed() };
    if addr != 0
        && unsafe { libc::dladdr(addr as *const libc::c_void, &mut info) } != 0
        && !info.dli_sname.is_null()
    {
        let name = unsafe { std::ffi::CStr::from_ptr(info.dli_sname) };
        return format!(
            "{}+{:#x}",
            name.to_string_lossy(),
            addr - info.dli_saddr as usize
        );
    }
    format!("{addr:#x}")
}

/// `dladdr` is POSIX; elsewhere every address is its own label.
#[cfg(not(unix))]
pub(crate) fn symbol(addr: usize) -> String {
    format!("{addr:#x}")
}
