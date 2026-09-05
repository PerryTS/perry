//! Heap census (`PERRY_GC_CENSUS=<path>`): an env-gated, off-by-default
//! observer that classifies the whole heap at the mark-complete point of a
//! FULL collection and appends one JSON line per census to `<path>`.
//!
//! Why inside the cycle: after a sweep, a dead nursery object keeps its
//! header (the sweep resets whole blocks, it does not free objects), so a
//! post-collection walk cannot tell live from dead. Inside a SYNCHRONOUS
//! full cycle the mark bits are authoritative and nothing moves. Two passes:
//!
//! 1. at the end of mark propagation (`step_mark_propagation`) the reachable
//!    set is snapshotted — every header carrying `GC_FLAG_MARKED` or
//!    `GC_FLAG_PINNED`. For a synchronous full cycle that is exactly
//!    reachability from the precise roots (weak processing marks nothing);
//! 2. at sweep entry (`step_sweep`) every object is classified: marked in
//!    pass 1 → **live**; marked only now → **late-marked** (retained by the
//!    block-persistence window that `step_block_persistence` resurrects —
//!    reachable from nothing, kept because a register might still hold it);
//!    unmarked → **dead**. Only the live share is broken down by type/class.
//!
//! Minor cycles do not mark the old generation and budgeted cycles mark
//! allocate-black with a snapshot-at-the-beginning, so only synchronous full
//! cycles are eligible; an arm set during another kind of cycle stays set.
//!
//! Triggers (both require the env var):
//! - an explicit `gc()` / `js_gc_collect` arms a census for the full cycle it
//!   runs (fixtures, tests);
//! - `SIGUSR2` sets a flag that the event loop's wait (`js_wait_for_event`)
//!   turns into an explicit full collection on the main thread (long-running
//!   apps such as a TUI).
//!
//! Everything here is read-only with respect to the JS heap. The walk itself
//! must not allocate on the JS heap (it runs inside the collector); it uses
//! Rust-owned buffers only. When the env var is unset nothing is installed,
//! nothing is armed, and the only residual cost is one relaxed atomic load
//! per event-loop wait plus one thread-local read per full cycle.

use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

static CENSUS_PATH: OnceLock<Option<String>> = OnceLock::new();
static SIGNAL_PENDING: AtomicBool = AtomicBool::new(false);
static SIGNAL_INSTALLED: AtomicBool = AtomicBool::new(false);
static MAIN_THREAD: OnceLock<std::thread::ThreadId> = OnceLock::new();

thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
    static SEQ: Cell<u32> = const { Cell::new(0) };
    static LABEL: RefCell<&'static str> = const { RefCell::new("manual") };
}

#[cfg(test)]
thread_local! {
    /// Test-only per-thread override of the output path, so a unit test can
    /// enable the census without touching the process env (the env read is a
    /// process-wide OnceLock that any earlier collection would latch).
    static TEST_PATH_OVERRIDE: RefCell<Option<&'static str>> = const { RefCell::new(None) };
}

/// Test-only: pin the census output path for this thread (leaked `&'static`
/// on purpose — tests hand in a handful of paths per process).
#[cfg(test)]
pub(crate) fn test_set_census_path(path: Option<String>) {
    let leaked: Option<&'static str> = path.map(|p| Box::leak(p.into_boxed_str()) as &'static str);
    TEST_PATH_OVERRIDE.with(|c| *c.borrow_mut() = leaked);
}

/// Test-only: is a census armed on this thread?
#[cfg(test)]
pub(crate) fn test_is_armed() -> bool {
    ARMED.with(|c| c.get())
}

/// The census output path, read once from `PERRY_GC_CENSUS`. `None` (the
/// default) disables every other entry point in this module.
pub(crate) fn census_path() -> Option<&'static str> {
    #[cfg(test)]
    if let Some(p) = TEST_PATH_OVERRIDE.with(|c| *c.borrow()) {
        return Some(p);
    }
    CENSUS_PATH
        .get_or_init(|| {
            std::env::var("PERRY_GC_CENSUS")
                .ok()
                .filter(|s| !s.is_empty())
        })
        .as_deref()
}

/// Is `PERRY_GC_CENSUS` set?
pub fn gc_census_enabled() -> bool {
    census_path().is_some()
}

/// Arm a census for the next full collection on this thread.
pub(crate) fn census_arm(label: &'static str) {
    if !gc_census_enabled() {
        return;
    }
    // An arm already pending keeps its label (the signal path arms with
    // "signal" and then runs `js_gc_collect`, which arms again as "manual").
    if ARMED.with(|c| c.replace(true)) {
        return;
    }
    LABEL.with(|l| *l.borrow_mut() = label);
}

/// Called from `gc_init` (the production entrypoint runs it on the main
/// thread first): remember the main thread and install the `SIGUSR2`
/// trigger. No-op unless the env var is set.
pub(crate) fn census_on_gc_init() {
    if !gc_census_enabled() {
        return;
    }
    let _ = MAIN_THREAD.set(std::thread::current().id());
    install_signal_handler();
}

#[cfg(unix)]
extern "C" fn census_signal_handler(_sig: libc::c_int) {
    // Async-signal-safe: only the atomic store. The event loop does the work.
    SIGNAL_PENDING.store(true, Ordering::Release);
}

#[cfg(unix)]
fn install_signal_handler() {
    if SIGNAL_INSTALLED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    // SAFETY: standard sigaction install with a handler that only stores an
    // atomic; SA_RESTART so an interrupted read/wait resumes.
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = census_signal_handler as *const () as usize;
        sa.sa_flags = libc::SA_RESTART;
        libc::sigemptyset(&mut sa.sa_mask);
        libc::sigaction(libc::SIGUSR2, &sa, std::ptr::null_mut());
    }
}

#[cfg(not(unix))]
fn install_signal_handler() {}

/// Another owner restored `SIG_DFL` on `signal` (the `process.on(sig)` /
/// `off` support does that when the last JS listener goes away — and the
/// compiled claude-code TUI registers-then-removes a `SIGUSR2` listener at
/// startup). Re-install the census trigger so the signal keeps meaning
/// "census" instead of "terminate". No-op unless the census is enabled.
#[cfg(unix)]
pub(crate) fn census_after_signal_disposition_reset(signal: libc::c_int) {
    if signal != libc::SIGUSR2 || !gc_census_enabled() {
        return;
    }
    SIGNAL_INSTALLED.store(false, Ordering::Release);
    install_signal_handler();
}

/// Event-loop poll: one relaxed load when idle. On a pending signal, the
/// main thread arms a census and runs an explicit full collection.
#[inline]
pub fn census_poll_signal() {
    if SIGNAL_PENDING.load(Ordering::Relaxed) {
        census_service_signal();
    }
}

#[cold]
fn census_service_signal() {
    if MAIN_THREAD.get() != Some(&std::thread::current().id()) {
        return;
    }
    SIGNAL_PENDING.store(false, Ordering::Release);
    census_arm("signal");
    super::js_gc_collect();
}

thread_local! {
    /// Pass-1 snapshot: sorted header addresses that were marked when mark
    /// propagation finished (see the module docs).
    static PASS1_MARKED: RefCell<Option<Vec<usize>>> = const { RefCell::new(None) };
}

#[inline]
fn header_is_marked(header: *const GcHeader) -> bool {
    // SAFETY: caller hands out headers of walkable objects inside mapped
    // blocks while the collector owns the heap.
    unsafe { (*header).gc_flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) != 0 }
}

/// Pass 1: end of mark propagation of a SYNCHRONOUS full cycle. Snapshots
/// the reachable set so the sweep-entry pass can tell reachability from
/// block-persistence retention. No-op unless armed.
pub(super) fn census_pass1_if_armed() {
    if !ARMED.with(|c| c.get()) {
        return;
    }
    let mut marked: Vec<usize> = Vec::with_capacity(1 << 16);
    crate::arena::arena_walk_objects_with_block_index(|header_ptr, _| {
        if header_is_marked(header_ptr as *const GcHeader) {
            marked.push(header_ptr as usize);
        }
    });
    MALLOC_STATE.with(|s| {
        for &header in &s.borrow().objects {
            if !header.is_null() && header_is_marked(header) {
                marked.push(header as usize);
            }
        }
    });
    marked.sort_unstable();
    PASS1_MARKED.with(|p| *p.borrow_mut() = Some(marked));
}

/// Pass 2: sweep entry of the same synchronous full cycle (all marks final,
/// nothing swept, block persistence already applied). Consumes the arm.
pub(super) fn census_take_if_armed_at_full_sweep_start() {
    if !ARMED.with(|c| c.replace(false)) {
        return;
    }
    let label = LABEL.with(|l| *l.borrow());
    let pass1 = PASS1_MARKED.with(|p| p.borrow_mut().take());
    take_census(label, pass1);
}

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

const SLOT_KINDS: [&str; 12] = [
    "pointer",
    "string",
    "short_string",
    "int32",
    "double",
    "undefined",
    "null",
    "bool",
    "hole_tdz",
    "bigint",
    "handle",
    "other",
];

#[inline]
fn slot_kind(bits: u64) -> usize {
    use crate::value::*;
    match bits & TAG_MASK {
        POINTER_TAG => 0,
        STRING_TAG => 1,
        SHORT_STRING_TAG => 2,
        INT32_TAG => 3,
        BIGINT_TAG => 9,
        JS_HANDLE_TAG => 10,
        TAG_MARKER => match bits {
            TAG_UNDEFINED => 5,
            TAG_NULL => 6,
            TAG_TRUE | TAG_FALSE => 7,
            TAG_HOLE | TAG_TDZ => 8,
            _ => 11,
        },
        _ => {
            // A raw double, or a raw 48-bit pointer stored untagged.
            if (0x1000..=0x0000_FFFF_FFFF_FFFF).contains(&bits) {
                0
            } else {
                4
            }
        }
    }
}

#[inline]
fn size_bucket(n: usize) -> usize {
    // 0 => <=16, 1 => <=32, ... doubling; index = ceil(log2(n)) - 4, clamped.
    let mut b = 16usize;
    let mut i = 0usize;
    while b < n && i < 20 {
        b <<= 1;
        i += 1;
    }
    i
}

fn bucket_label(i: usize) -> String {
    format!("<={}", 16usize << i)
}

#[derive(Clone, Copy, Default)]
struct Acc {
    count: u64,
    bytes: u64,
}

impl Acc {
    #[inline]
    fn add(&mut self, bytes: usize) {
        self.count += 1;
        self.bytes += bytes as u64;
    }
}

#[derive(Default)]
struct ClassAcc {
    count: u64,
    bytes: u64,
    slot_capacity: u64,
    slot_live: u64,
    meta: u64,
    unshaped: u64,
}

#[derive(Default)]
struct Census {
    // per space: 0..5 arenas (walk order), 5 = malloc
    space_live: [Acc; 6],
    space_dead: [Acc; 6],
    space_walked_bytes: [u64; 6],
    space_stub_live: [Acc; 6],
    space_stub_dead: [Acc; 6],
    space_late: [Acc; 6],
    type_live: [Acc; GC_TYPE_MAX as usize + 1],
    type_dead: [Acc; GC_TYPE_MAX as usize + 1],
    type_late: [Acc; GC_TYPE_MAX as usize + 1],
    /// Pass-1 reachable set (sorted header addresses); `None` = trust marks.
    pass1: Option<Vec<usize>>,
    size_hist: [Acc; 21],
    classes: std::collections::HashMap<u32, ClassAcc>,
    obj_slot_tags: [u64; 12],
    arr_slot_tags: [u64; 12],
    clo_slot_tags: [u64; 12],
    // strings
    str_payload: u64,
    str_capacity: u64,
    str_interned: Acc,
    str_buckets: [Acc; 21],
    str_buckets_payload: [u64; 21],
    // arrays
    arr_length: u64,
    arr_capacity: u64,
    arr_shape_keys: Acc,
    arr_buckets: [Acc; 21],
    // closures
    clo_captures: u64,
    // objects
    obj_meta: u64,
    /// ShapeId stamp of every live shaped object (duplicates included; sorted
    /// and deduplicated once the walk is over). Feeds the shape table's
    /// "carried by a live object" rows (#9706).
    live_shape_ids: Vec<u32>,
}

const SPACE_NAMES: [&str; 6] = [
    "nursery_eden",
    "survivor0",
    "survivor1",
    "longlived",
    "old",
    "malloc",
];

impl Census {
    /// Classify one header. `space` indexes `SPACE_NAMES`.
    unsafe fn visit(&mut self, header: *const GcHeader, space: usize) {
        let flags = (*header).gc_flags;
        let obj_type = (*header).obj_type;
        let size = (*header).size as usize;
        self.space_walked_bytes[space] += size as u64;
        let marked = flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) != 0;
        if flags & GC_FLAG_FORWARDED != 0 {
            if marked {
                self.space_stub_live[space].add(size);
            } else {
                self.space_stub_dead[space].add(size);
            }
            return;
        }
        let t = obj_type as usize;
        if !marked {
            self.space_dead[space].add(size);
            if t <= GC_TYPE_MAX as usize {
                self.type_dead[t].add(size);
            }
            return;
        }
        if let Some(pass1) = self.pass1.as_ref() {
            if pass1.binary_search(&(header as usize)).is_err() {
                // Marked only after mark propagation finished: retained by
                // block persistence, unreachable from the roots.
                self.space_late[space].add(size);
                if t <= GC_TYPE_MAX as usize {
                    self.type_late[t].add(size);
                }
                return;
            }
        }
        self.space_live[space].add(size);
        if t > GC_TYPE_MAX as usize {
            return;
        }
        self.type_live[t].add(size);
        self.size_hist[size_bucket(size)].add(size);
        let user = (header as *const u8).add(GC_HEADER_SIZE);
        match obj_type {
            GC_TYPE_OBJECT => {
                self.visit_object(user as *const crate::object::ObjectHeader, size, flags)
            }
            GC_TYPE_ARRAY => {
                self.visit_array(user as *const crate::array::ArrayHeader, size, flags)
            }
            GC_TYPE_STRING => self.visit_string(user as *const crate::StringHeader, size, flags),
            GC_TYPE_CLOSURE => {
                self.visit_closure(user as *const crate::closure::ClosureHeader, size)
            }
            _ => {}
        }
    }

    unsafe fn visit_object(
        &mut self,
        obj: *const crate::object::ObjectHeader,
        size: usize,
        _flags: u8,
    ) {
        let header_bytes = GC_HEADER_SIZE + std::mem::size_of::<crate::object::ObjectHeader>();
        let slot_capacity = size.saturating_sub(header_bytes) / 8;
        let class_id = (*obj).class_id;
        let entry = self.classes.entry(class_id).or_default();
        entry.count += 1;
        entry.bytes += size as u64;
        entry.slot_capacity += slot_capacity as u64;
        if !(*obj).meta.is_null() {
            entry.meta += 1;
            self.obj_meta += 1;
        }
        let live = match crate::object::shapes::object_shape_descriptor(obj) {
            Some(d) => {
                self.live_shape_ids
                    .push(crate::object::shapes::object_shape_stamp(obj));
                (d.live_inline_slot_count as usize).min(slot_capacity)
            }
            None => {
                entry.unshaped += 1;
                0
            }
        };
        entry.slot_live += live as u64;
        let slots = (obj as *const u8).add(std::mem::size_of::<crate::object::ObjectHeader>())
            as *const u64;
        for i in 0..live {
            self.obj_slot_tags[slot_kind(*slots.add(i))] += 1;
        }
    }

    unsafe fn visit_array(
        &mut self,
        arr: *const crate::array::ArrayHeader,
        size: usize,
        flags: u8,
    ) {
        let header_bytes = GC_HEADER_SIZE + std::mem::size_of::<crate::array::ArrayHeader>();
        let slot_capacity = size.saturating_sub(header_bytes) / 8;
        let length = ((*arr).length as usize).min(slot_capacity);
        self.arr_length += length as u64;
        self.arr_capacity += slot_capacity as u64;
        self.arr_buckets[size_bucket(size)].add(size);
        if flags & GC_FLAG_SHAPE_SHARED != 0 {
            self.arr_shape_keys.add(size);
        }
        let elems =
            (arr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const u64;
        for i in 0..length {
            self.arr_slot_tags[slot_kind(*elems.add(i))] += 1;
        }
    }

    unsafe fn visit_string(&mut self, s: *const crate::StringHeader, size: usize, flags: u8) {
        let byte_len = (*s).byte_len as usize;
        let payload = byte_len.min(size);
        self.str_payload += payload as u64;
        self.str_capacity += ((*s).capacity as usize).min(size) as u64;
        let b = size_bucket(size);
        self.str_buckets[b].add(size);
        self.str_buckets_payload[b] += payload as u64;
        if flags & GC_FLAG_INTERNED != 0 {
            self.str_interned.add(size);
        }
    }

    unsafe fn visit_closure(&mut self, c: *const crate::closure::ClosureHeader, size: usize) {
        let header_bytes = GC_HEADER_SIZE + std::mem::size_of::<crate::closure::ClosureHeader>();
        let slot_capacity = size.saturating_sub(header_bytes) / 8;
        let captures = ((*c).capture_count as usize).min(slot_capacity);
        self.clo_captures += captures as u64;
        let caps = (c as *const u8).add(std::mem::size_of::<crate::closure::ClosureHeader>())
            as *const u64;
        for i in 0..captures {
            self.clo_slot_tags[slot_kind(*caps.add(i))] += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Side tables
// ---------------------------------------------------------------------------

/// One runtime-owned table outside the GC heap: `(name, entries, est_bytes)`.
pub(crate) type SideTableRow = (&'static str, usize, usize);

/// Estimated bytes held by a hashbrown table with `capacity()` == `capacity`
/// and `size_of::<(K, V)>()` == `kv`: one bucket-sized slot plus one control
/// byte per bucket.
///
/// `HashMap::capacity()` is already the post-load-factor figure — the number
/// of elements the table holds without reallocating, i.e. `buckets * 7/8` —
/// so recovering the bucket count is `capacity * 8/7`, and the rounding up to
/// a power of two only matters for the small tables where that product is not
/// already one. An earlier version added 1 before rounding, which pushed every
/// exactly-sized table to the NEXT power of two and reported up to 2x the real
/// storage; `debug_assert`s below pin both ends of that mistake.
pub(crate) fn hash_table_bytes(capacity: usize, kv: usize) -> usize {
    if capacity == 0 {
        return 0;
    }
    let buckets = (capacity * 8 / 7).max(1).next_power_of_two();
    debug_assert!(
        buckets * 7 / 8 >= capacity,
        "bucket count must actually hold the reported capacity"
    );
    debug_assert!(
        buckets / 2 * 7 / 8 < capacity || buckets <= 8,
        "bucket count must be the SMALLEST power of two that holds it —          an off-by-one before `next_power_of_two` doubles every estimate"
    );
    buckets * (kv + 1) + 16
}

/// Estimated bytes of a `HashMap`'s table storage.
pub(crate) fn map_bytes<K, V, S>(m: &std::collections::HashMap<K, V, S>) -> usize {
    hash_table_bytes(m.capacity(), std::mem::size_of::<(K, V)>())
}

/// Estimated bytes of a `HashSet`'s table storage.
pub(crate) fn set_bytes<T, S>(s: &std::collections::HashSet<T, S>) -> usize {
    hash_table_bytes(s.capacity(), std::mem::size_of::<T>())
}

/// Bytes of a `Vec`'s heap buffer (capacity, not length).
pub(crate) fn vec_bytes<T>(v: &Vec<T>) -> usize {
    v.capacity() * std::mem::size_of::<T>()
}

fn side_tables() -> Vec<SideTableRow> {
    let mut rows: Vec<SideTableRow> = Vec::new();
    rows.extend(crate::builtins::function_registries_census());
    rows.extend(crate::closure::closure_registry_census());
    rows.extend(crate::closure::closure_side_table_census());
    rows.extend(crate::object::shapes::shape_table_census());
    rows.extend(crate::object::class_registry_census());
    rows.extend(crate::object::object_tables_census());
    rows.extend(crate::object::pic_slot_census());
    rows.extend(super::roots::stack_map_index_census());
    let (slots, bytes) = crate::string::intern_table_census();
    rows.push(("string.intern_table(fixed)", slots, bytes));
    rows.extend(super::malloc_state_census());
    rows.extend(crate::arena::page_meta_census());
    rows.extend(super::barrier::census_rows::barrier_tables_census());
    rows.extend(crate::module_require::path_registry_census());
    rows.extend(crate::timer::timer_tables_census());
    rows.push(crate::symbol::symbol_registry_census());
    let (masks, typed) = super::layout_tables::per_object_layout_table_sizes();
    rows.push(("gc.layout_slot_masks", masks, masks * 24));
    rows.push(("gc.typed_layouts", typed, typed * 24));
    rows.push((
        "gc.external_side_live_bytes(map/set/tape)",
        0,
        super::policy::external_side_live_bytes(),
    ));
    rows
}

// ---------------------------------------------------------------------------
// Process-level numbers
// ---------------------------------------------------------------------------

#[cfg(target_vendor = "apple")]
fn phys_footprint_bytes() -> Option<u64> {
    // SAFETY: proc_pid_rusage fills a caller-provided rusage_info_v4.
    unsafe {
        let mut info: libc::rusage_info_v4 = std::mem::zeroed();
        let rc = libc::proc_pid_rusage(
            libc::getpid(),
            libc::RUSAGE_INFO_V4,
            &mut info as *mut libc::rusage_info_v4 as *mut libc::rusage_info_t,
        );
        if rc == 0 {
            Some(info.ri_phys_footprint)
        } else {
            None
        }
    }
}

#[cfg(not(target_vendor = "apple"))]
fn phys_footprint_bytes() -> Option<u64> {
    None
}

// `libmimalloc-sys` is an Apple-only dependency of this crate (the OS-tag
// retag is what pulls it in), so the stats call is Apple-only too.
#[cfg(all(feature = "alloc-mimalloc", target_vendor = "apple"))]
fn mimalloc_info() -> serde_json::Value {
    let (mut e, mut u, mut s, mut rss, mut prss, mut commit, mut pcommit, mut pf) = (
        0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize,
    );
    // SAFETY: plain out-params.
    unsafe {
        libmimalloc_sys::mi_process_info(
            &mut e,
            &mut u,
            &mut s,
            &mut rss,
            &mut prss,
            &mut commit,
            &mut pcommit,
            &mut pf,
        );
    }
    serde_json::json!({
        "current_rss": rss,
        "peak_rss": prss,
        "current_commit": commit,
        "peak_commit": pcommit,
    })
}

#[cfg(not(all(feature = "alloc-mimalloc", target_vendor = "apple")))]
fn mimalloc_info() -> serde_json::Value {
    serde_json::Value::Null
}

// ---------------------------------------------------------------------------
// The census itself
// ---------------------------------------------------------------------------

fn take_census(label: &str, pass1: Option<Vec<usize>>) {
    let Some(path) = census_path() else {
        return;
    };
    // The Rust-heap census first: the GC walk below allocates, and the point
    // of that census is what was resident before this collection started.
    #[cfg(feature = "alloc-census")]
    {
        crate::alloc_census::alloc_census_dump(label);
        crate::alloc_census::mimalloc_stats_print();
    }
    let started = Instant::now();
    let mut c = Census {
        pass1,
        ..Census::default()
    };

    // Arena regions: block-index bounds in walk order.
    let spaces = crate::arena::arena_space_census();
    let bounds: Vec<usize> = spaces.iter().map(|s| s.block_index_end).collect();
    crate::arena::arena_walk_objects_with_block_index(|header_ptr, block_idx| {
        let space = bounds.iter().position(|&end| block_idx < end).unwrap_or(4);
        // SAFETY: the walker hands out headers of walkable objects inside
        // mapped arena blocks; the collector owns the heap at this point.
        unsafe { c.visit(header_ptr as *const GcHeader, space) };
    });
    let malloc_headers: Vec<*mut GcHeader> = MALLOC_STATE.with(|s| s.borrow().objects.clone());
    for header in malloc_headers {
        if header.is_null() {
            continue;
        }
        // SAFETY: registered malloc GC objects stay mapped until swept.
        unsafe { c.visit(header as *const GcHeader, 5) };
    }

    // ---- assemble JSON ----
    let acc = |a: &Acc| serde_json::json!({"count": a.count, "bytes": a.bytes});
    let tags = |t: &[u64; 12]| -> serde_json::Value {
        let mut m = serde_json::Map::new();
        for (i, name) in SLOT_KINDS.iter().enumerate() {
            m.insert((*name).to_string(), serde_json::json!(t[i]));
        }
        serde_json::Value::Object(m)
    };

    let mut space_rows = Vec::new();
    for (i, name) in SPACE_NAMES.iter().enumerate() {
        let mut row = serde_json::json!({
            "space": name,
            "live": acc(&c.space_live[i]),
            "dead": acc(&c.space_dead[i]),
            "stub_live": acc(&c.space_stub_live[i]),
            "stub_dead": acc(&c.space_stub_dead[i]),
            "late_marked": acc(&c.space_late[i]),
            "walked_bytes": c.space_walked_bytes[i],
        });
        if i < 5 {
            let s = &spaces[i];
            row["blocks"] = serde_json::json!(s.blocks);
            row["tombstones"] = serde_json::json!(s.tombstones);
            row["capacity_bytes"] = serde_json::json!(s.capacity_bytes);
            row["used_bytes"] = serde_json::json!(s.used_bytes);
            row["object_starts_bytes"] = serde_json::json!(s.object_starts_bytes);
        }
        space_rows.push(row);
    }

    let mut type_rows = Vec::new();
    for t in 1..=GC_TYPE_MAX as usize {
        let name = gc_type_info(t as u8).map(|i| i.name).unwrap_or("?");
        if c.type_live[t].count == 0 && c.type_dead[t].count == 0 && c.type_late[t].count == 0 {
            continue;
        }
        type_rows.push(serde_json::json!({
            "type": name,
            "id": t,
            "live": acc(&c.type_live[t]),
            "dead": acc(&c.type_dead[t]),
            "late_marked": acc(&c.type_late[t]),
        }));
    }

    let mut class_rows: Vec<(u64, serde_json::Value)> = c
        .classes
        .iter()
        .map(|(id, a)| {
            let name = if *id == 0 {
                "Object(anon)".to_string()
            } else {
                crate::object::class_name_for_id(*id).unwrap_or_else(|| format!("class#{id}"))
            };
            (
                a.bytes,
                serde_json::json!({
                    "class_id": id,
                    "name": name,
                    "count": a.count,
                    "bytes": a.bytes,
                    "slot_capacity": a.slot_capacity,
                    "slot_live": a.slot_live,
                    "meta": a.meta,
                    "unshaped": a.unshaped,
                }),
            )
        })
        .collect();
    class_rows.sort_by(|a, b| b.0.cmp(&a.0));
    let class_count = class_rows.len();
    let (mut other_count, mut other_bytes) = (0u64, 0u64);
    let mut class_json = Vec::new();
    for (i, (bytes, row)) in class_rows.into_iter().enumerate() {
        if i < 400 {
            class_json.push(row);
        } else {
            other_bytes += bytes;
            other_count += row["count"].as_u64().unwrap_or(0);
        }
    }

    let hist = |h: &[Acc; 21]| -> Vec<serde_json::Value> {
        h.iter()
            .enumerate()
            .filter(|(_, a)| a.count > 0)
            .map(|(i, a)| serde_json::json!({"bucket": bucket_label(i), "count": a.count, "bytes": a.bytes}))
            .collect()
    };
    let str_hist: Vec<serde_json::Value> = c
        .str_buckets
        .iter()
        .enumerate()
        .filter(|(_, a)| a.count > 0)
        .map(|(i, a)| {
            serde_json::json!({"bucket": bucket_label(i), "count": a.count, "bytes": a.bytes, "payload_bytes": c.str_buckets_payload[i]})
        })
        .collect();

    c.live_shape_ids.sort_unstable();
    c.live_shape_ids.dedup();
    let mut side_rows = side_tables();
    side_rows.extend(crate::object::shapes::shape_table_liveness_census(
        &c.live_shape_ids,
    ));
    let side: Vec<serde_json::Value> = side_rows
        .into_iter()
        .map(|(n, e, b)| serde_json::json!({"table": n, "entries": e, "bytes": b}))
        .collect();
    let side_total: usize = side
        .iter()
        .map(|r| r["bytes"].as_u64().unwrap_or(0) as usize)
        .sum();

    let live_total: u64 = c.space_live.iter().map(|a| a.bytes).sum();
    let dead_total: u64 = c.space_dead.iter().map(|a| a.bytes).sum();
    let late_total: u64 = c.space_late.iter().map(|a| a.bytes).sum();
    let late_objects: u64 = c.space_late.iter().map(|a| a.count).sum();
    let pass1_present = c.pass1.is_some();
    let arena_capacity: usize = spaces.iter().map(|s| s.capacity_bytes).sum();
    let arena_used: usize = spaces.iter().map(|s| s.used_bytes).sum();
    let seq = SEQ.with(|s| {
        let v = s.get();
        s.set(v + 1);
        v
    });

    let doc = serde_json::json!({
        "perry_gc_census": 1,
        "seq": seq,
        "label": label,
        "pid": std::process::id(),
        "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
        "process": {
            "rss_bytes": crate::process::get_rss_bytes(),
            "phys_footprint_bytes": phys_footprint_bytes(),
            "mimalloc": mimalloc_info(),
        },
        "arena": {
            "capacity_bytes": arena_capacity,
            "used_bytes": arena_used,
            "free_list_bytes": crate::arena::arena_free_list_bytes(),
            "old_free_bytes": super::old_free_bytes(),
            "block_pool_bytes": crate::arena::block_pool_bytes(),
            "live_allocated_bytes_estimate": crate::arena::arena_live_allocated_bytes(),
            "spaces": space_rows,
        },
        "totals": {
            "live_bytes": live_total,
            "dead_bytes": dead_total,
            "side_table_bytes": side_total,
            "live_objects": c.space_live.iter().map(|a| a.count).sum::<u64>(),
            "dead_objects": c.space_dead.iter().map(|a| a.count).sum::<u64>(),
            "late_marked_bytes": late_total,
            "late_marked_objects": late_objects,
            "reachability_pass": pass1_present,
        },
        "by_type": type_rows,
        "objects": {
            "classes": class_count,
            "with_meta": c.obj_meta,
            "slot_tags": tags(&c.obj_slot_tags),
            "by_class": class_json,
            "other_classes": {"count": other_count, "bytes": other_bytes},
        },
        "strings": {
            "payload_bytes": c.str_payload,
            "capacity_bytes": c.str_capacity,
            "interned": acc(&c.str_interned),
            "by_size": str_hist,
        },
        "arrays": {
            "length_total": c.arr_length,
            "capacity_total": c.arr_capacity,
            "shape_keys_arrays": acc(&c.arr_shape_keys),
            "slot_tags": tags(&c.arr_slot_tags),
            "by_size": hist(&c.arr_buckets),
        },
        "closures": {
            "captures_total": c.clo_captures,
            "slot_tags": tags(&c.clo_slot_tags),
        },
        "size_histogram": hist(&c.size_hist),
        "side_tables": side,
    });

    let mut line = doc.to_string();
    line.push('\n');
    use std::io::Write;
    let res = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| f.write_all(line.as_bytes()));
    if let Err(e) = res {
        eprintln!("[gc-census] cannot write {path}: {e}");
    } else if gc_diag_enabled() {
        eprintln!(
            "[gc-census] seq={seq} label={label} live={live_total} late={late_total} dead={dead_total} side={side_total} -> {path}"
        );
    }
}
