//! Box runtime for mutable captured variables
//!
//! When a closure captures a variable that is modified (either in the closure
//! or in the outer scope), we need to store it in a heap-allocated "box" so
//! both scopes share the same storage location.

use std::alloc::{alloc, Layout};
use std::sync::atomic::{AtomicU64, Ordering};

static BOX_GET_NULL_COUNT: AtomicU64 = AtomicU64::new(0);
static BOX_SET_NULL_COUNT: AtomicU64 = AtomicU64::new(0);
static I32_BOX_GET_NULL_COUNT: AtomicU64 = AtomicU64::new(0);
static I32_BOX_SET_NULL_COUNT: AtomicU64 = AtomicU64::new(0);
static BOOL_BOX_GET_NULL_COUNT: AtomicU64 = AtomicU64::new(0);
static BOOL_BOX_SET_NULL_COUNT: AtomicU64 = AtomicU64::new(0);

// #7933 follow-up (async-state RSS accumulation) — release/reuse telemetry.
// `allocs` counts every `js_*box_alloc*` call, `pool_reuses` the subset served
// from the free pool instead of `std::alloc`, `releases` the cells parked by
// `js_*box_release`. `allocs - pool_reuses` is the number of cells ever
// malloc'd — the process-lifetime malloc residue the regression test gates on.
static BOX_ALLOC_COUNT: AtomicU64 = AtomicU64::new(0);
static BOX_POOL_REUSE_COUNT: AtomicU64 = AtomicU64::new(0);
static BOX_RELEASE_COUNT: AtomicU64 = AtomicU64::new(0);
// Diagnostic-only (#8208/#8213 tuning): how often the global quarantine and
// per-activation scopes publish, and how many cells they publish in total.
// A pool whose high-water mark is 41x one batch's working set is a publication-
// frequency question, not a pool-size question.
static BOX_FLUSH_COUNT: AtomicU64 = AtomicU64::new(0);
static BOX_FLUSH_PUBLISHED: AtomicU64 = AtomicU64::new(0);
static BOX_ACTIVATION_PUBLISH_COUNT: AtomicU64 = AtomicU64::new(0);

/// Snapshot of the release/reuse counters: `(allocs, pool_reuses, releases)`.
/// Sums all three box kinds.
pub fn box_release_stats() -> (u64, u64, u64) {
    (
        BOX_ALLOC_COUNT.load(Ordering::Relaxed),
        BOX_POOL_REUSE_COUNT.load(Ordering::Relaxed),
        BOX_RELEASE_COUNT.load(Ordering::Relaxed),
    )
}

/// `PERRY_GC_DIAG=1`: one line at process exit with the box release/reuse
/// counters and the surviving registry populations. `resident = allocs -
/// pool_reuses` is the number of cells that cost a real `std::alloc`
/// allocation over the process lifetime — the quantity that grew linearly
/// with completed async activations before the #7933 follow-up. Emitted from
/// the same exit funnel as the other GC diagnostics
/// (`js_gc_release_current_thread_collection_side_allocations`).
pub fn report_box_stats_at_exit() {
    if !crate::gc::gc_diag_enabled() {
        return;
    }
    static EMITTED: AtomicU64 = AtomicU64::new(0);
    if EMITTED.swap(1, Ordering::SeqCst) != 0 {
        return;
    }
    let (allocs, reuses, releases) = box_release_stats();
    let (reg, i32_reg, bool_reg) = (
        BOX_REGISTRY.with(|r| r.borrow().len()),
        I32_BOX_REGISTRY.with(|r| r.borrow().len()),
        BOOL_BOX_REGISTRY.with(|r| r.borrow().len()),
    );
    eprintln!(
        "[box-stats] allocs={allocs} pool_reuses={reuses} releases={releases} \
         resident_cells={} registry_len={reg} i32_registry_len={i32_reg} \
         bool_registry_len={bool_reg} flushes={} activation_publishes={} published={}",
        allocs - reuses,
        BOX_FLUSH_COUNT.load(Ordering::Relaxed),
        BOX_ACTIVATION_PUBLISH_COUNT.load(Ordering::Relaxed),
        BOX_FLUSH_PUBLISHED.load(Ordering::Relaxed),
    );
}

/// A box is simply a heap-allocated JSValue bit slot.
#[repr(C)]
pub struct Box {
    pub value: u64,
}

#[repr(C, align(8))]
pub struct I32Box {
    pub value: i32,
}

#[repr(C, align(8))]
pub struct BoolBox {
    pub value: bool,
}

crate::perry_thread_local! {
    /// Registry of every active box pointer. GC traces the contained
    /// JSValue bits so that NaN-boxed heap pointers stored in boxes (e.g.
    /// the generator state machine's iter object held in `__iter`'s
    /// mutable-capture box) keep the referenced heap object alive
    /// across collections. Without this, captures stored as raw box
    /// pointers in closure capture slots fail the `valid_ptrs.contains`
    /// check during `trace_closure` (boxes come from `std::alloc::alloc`
    /// directly, not the GC arena), so the box pointer is never marked
    /// AND the JSValue bits inside are never scanned — heap objects
    /// referenced only through box-captures can be swept mid-await.
    pub(crate) static BOX_REGISTRY: std::cell::RefCell<crate::fast_hash::PtrHashSet<usize>> =
        // Pre-size for promise-heavy workloads: `promise_all_chains`
        // allocates ~150 k boxes per kernel run (one per closure
        // mutable capture). Starting at 128 k buckets (~2 MB) covers
        // the full working set in one alloc — without it, hashbrown
        // rehashes from 0 → 256 k buckets across the alloc history,
        // showing up as ~3 % CPU in `hash_one` / `reserve_rehash`.
        std::cell::RefCell::new(std::collections::HashSet::with_capacity_and_hasher(
            128 * 1024,
            crate::fast_hash::PtrHasher,
        ));
    pub(crate) static I32_BOX_REGISTRY: std::cell::RefCell<crate::fast_hash::PtrHashSet<usize>> =
        std::cell::RefCell::new(std::collections::HashSet::with_capacity_and_hasher(
            16 * 1024,
            crate::fast_hash::PtrHasher,
        ));
    pub(crate) static BOOL_BOX_REGISTRY: std::cell::RefCell<crate::fast_hash::PtrHashSet<usize>> =
        std::cell::RefCell::new(std::collections::HashSet::with_capacity_and_hasher(
            16 * 1024,
            crate::fast_hash::PtrHasher,
        ));
}

/// Number of slots in each registry's direct-mapped positive cache. Eight
/// covers the working set that matters: the async-to-generator state machine
/// re-reads the same handful of boxes (`__gen_state`, `__gen_done`,
/// `__gen_executing`, plus the activation's body locals) on every step, and
/// activations run one at a time.
const BOX_PTR_CACHE_SLOTS: usize = 8;

type BoxPtrCache = crate::tls_hot::HotKey<[std::cell::Cell<usize>; BOX_PTR_CACHE_SLOTS]>;
type BoxFreeHead = crate::tls_hot::HotKey<std::cell::Cell<usize>>;

crate::perry_thread_local! {
    /// Direct-mapped **positive** cache over `BOX_REGISTRY`.
    ///
    /// `js_box_get`/`js_box_set` validate their operand against the registry on
    /// every access (perry#4898), and that hash probe is the single largest leaf
    /// in Perry's async machinery — the transform boxes every body local of an
    /// `async` function, so a state machine pays one probe per local read and
    /// one per write. Measured on a promise-only kernel (24 000 activations,
    /// 48 000 awaits): `is_registered_{,i32_,bool_}box_ptr` were 8.2 % + 5.9 %
    /// + 5.5 % of leaf samples.
    ///
    /// ## Why caching only positives is sound
    ///
    /// Box-cell memory is **never returned to the allocator**: an address
    /// minted by `js_*box_alloc*` is a box cell for the life of the thread —
    /// live in the registry, or (since the #7933 follow-up) parked in the
    /// release quarantine/free pool, but never recycled into a non-box
    /// allocation. `js_*box_release` removes a cell from the registry AND
    /// evicts it from this cache (`box_ptr_cache_evict`), so a cache hit
    /// still implies "currently registered": the only writer that removes a
    /// registry entry clears the matching cache slot in the same call, on
    /// the same thread. A hit is therefore exactly as authoritative as the
    /// probe it replaces.
    ///
    /// A **negative** cache would NOT be sound — an address that is not a box
    /// today can be minted as one tomorrow — so a miss always falls through to
    /// the hash set, and only a confirmed positive is recorded. That keeps the
    /// perry#4898 rejection (a read-only `__TEXT.__cstring` address that passes
    /// every structural check) exactly as strict as before.
    ///
    /// Thread-local like the registry it fronts: a box minted on another thread
    /// is not in this thread's registry, and never enters this thread's cache.
    static BOX_PTR_CACHE: [std::cell::Cell<usize>; BOX_PTR_CACHE_SLOTS] =
        const { [const { std::cell::Cell::new(0) }; BOX_PTR_CACHE_SLOTS] };
    static I32_BOX_PTR_CACHE: [std::cell::Cell<usize>; BOX_PTR_CACHE_SLOTS] =
        const { [const { std::cell::Cell::new(0) }; BOX_PTR_CACHE_SLOTS] };
    static BOOL_BOX_PTR_CACHE: [std::cell::Cell<usize>; BOX_PTR_CACHE_SLOTS] =
        const { [const { std::cell::Cell::new(0) }; BOX_PTR_CACHE_SLOTS] };
}

crate::perry_thread_local! {
    /// #7933 follow-up: reusable cells for each box kind, and the quarantine
    /// feeding them.
    ///
    /// `js_*box_release` (emitted at a plain-async activation's terminal
    /// states, ONLY for cells the transform's escape analysis proved no
    /// closure can observe — `perry-transform/src/generator/box_release.rs`)
    /// clears the cell, removes it from its registry, and parks the address
    /// in the current async-step invocation's release scope. Once that
    /// invocation returns, its cells can move directly to the free list if
    /// no resume for the same activation remains queued. If a duplicate
    /// resume is still queued (or release happens outside an async step), the
    /// cells fall back to the process-turn QUARANTINE. That quarantine drains
    /// at the outermost empty-queue microtask-pump boundary
    /// (`flush_released_boxes`, called from `promise/microtasks.rs`).
    /// `js_*box_alloc*` pops the pool before touching `std::alloc`.
    ///
    /// ## Why the boundary, and why this is sound
    ///
    /// A released cell's address can still be REACHED (not legitimately
    /// read) by one thing: a duplicate resume of the already-terminal
    /// activation, which can only exist as a `Task::AsyncStep` already
    /// sitting in this thread's TASK_QUEUE (every suspend registers the step
    /// on a native, settle-once Promise, so each registration fires at most
    /// once; user thenables are assimilated first and cannot double-fire the
    /// step). While parked, the address is INERT: it is out of the registry,
    /// so `js_box_set` drops the write and `js_box_get` returns `undefined`,
    /// which routes a stray resume into the dispatch loop's default
    /// done-arm — byte-for-byte the behavior of the pre-existing cleared-cell
    /// path. By the time the queue has fully drained, no reference to the
    /// activation's cells exists anywhere, so reusing the address is
    /// unobservable.
    ///
    /// Memory safety is unconditional either way: cells only ever move
    /// between the registry, the quarantine and the pool — they are never
    /// handed back to the allocator — so every address ever minted by
    /// `js_*box_alloc*` stays a valid box cell for the life of the thread.
    /// That preserves the two properties the never-free design bought:
    /// perry#4898's rejection of foreign pointers (an address is either a
    /// live registered cell or an inert parked one) and #7906's positive
    /// pointer cache ("was a box" can never become "is another object").
    ///
    /// NOT a GC root: parked cells are cleared before parking, and the
    /// addresses themselves are `std::alloc` memory, not GC-heap pointers
    /// (see `scripts/gc_runtime_root_holders.json`).
    /// Head of the per-kind INTRUSIVE free list; 0 is the empty list.
    ///
    /// A free cell's own 8 bytes hold the address of the next free cell, so
    /// the reuse pool costs **zero** bytes of side table. That is not a
    /// micro-optimisation: a `Vec<usize>` pool is one 8-byte slot per cell on
    /// top of the cell, and the pool's high-water mark is ~330 cells per unit
    /// of PEAK CONCURRENCY (measured: `resident_cells / SIZE` is 329-334
    /// across a 16x sweep), held for the life of the thread. At SIZE=200 that
    /// side table was ~1 MB and made small async workloads a net RSS
    /// REGRESSION; threading the list through the cells removes it entirely.
    ///
    /// Overwriting the cell is why this list holds only cells that are PAST
    /// the quarantine. A quarantined cell must keep the parked terminal value
    /// a stray duplicate resume reads (`-1` / `true` / `undefined`); once
    /// the owning step has returned with no matching resume queued — or
    /// `flush_released_boxes` has run with the whole task queue empty — no such
    /// resume can exist, so the bytes are free to become a link.
    static BOX_FREE_HEAD: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static I32_BOX_FREE_HEAD: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static BOOL_BOX_FREE_HEAD: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static BOX_RELEASE_QUARANTINE: std::cell::RefCell<Vec<usize>> =
        std::cell::RefCell::new(Vec::new());
    static I32_BOX_RELEASE_QUARANTINE: std::cell::RefCell<Vec<usize>> =
        std::cell::RefCell::new(Vec::new());
    static BOOL_BOX_RELEASE_QUARANTINE: std::cell::RefCell<Vec<usize>> =
        std::cell::RefCell::new(Vec::new());
    /// Nested because a step body may synchronously enter another async
    /// activation, which may itself re-enter the microtask runner. Each scope
    /// owns only the cells released by that one step invocation.
    static ASYNC_BOX_RELEASE_SCOPES: std::cell::RefCell<Vec<ReleasedBoxBatch>> =
        const { std::cell::RefCell::new(Vec::new()) };
    /// Empty batches retain their three Vec buffers here after a scope
    /// finishes. A hot server therefore allocates at most one buffer cohort
    /// per synchronous nesting depth, not three fresh Vecs per activation.
    static ASYNC_BOX_RELEASE_BATCH_POOL: std::cell::RefCell<Vec<ReleasedBoxBatch>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[derive(Default)]
struct ReleasedBoxBatch {
    boxes: Vec<usize>,
    i32_boxes: Vec<usize>,
    bool_boxes: Vec<usize>,
}

impl ReleasedBoxBatch {
    fn is_empty(&self) -> bool {
        self.boxes.is_empty() && self.i32_boxes.is_empty() && self.bool_boxes.is_empty()
    }
}

#[derive(Copy, Clone)]
enum ReleasedBoxKind {
    Box,
    I32,
    Bool,
}

/// One invocation of a compiler-generated async step. Releases stay local to
/// this scope until the step returns, so they cannot be reused by allocations
/// made later in the same terminal path. Nested async calls get nested scopes.
pub(crate) struct AsyncBoxReleaseScope {
    depth: usize,
    finished: bool,
}

pub(crate) fn begin_async_box_release_scope() -> AsyncBoxReleaseScope {
    let batch = ASYNC_BOX_RELEASE_BATCH_POOL
        .with(|pool| pool.borrow_mut().pop())
        .unwrap_or_default();
    debug_assert!(batch.is_empty());
    let depth = ASYNC_BOX_RELEASE_SCOPES.with(|scopes| {
        let mut scopes = scopes.borrow_mut();
        scopes.push(batch);
        scopes.len()
    });
    AsyncBoxReleaseScope {
        depth,
        finished: false,
    }
}

impl AsyncBoxReleaseScope {
    /// Finish the step invocation. `activation_is_quiescent` is true only
    /// after the caller has re-read the possibly moved step closure and
    /// proved TASK_QUEUE has no `AsyncStep` for it. Otherwise preserve the
    /// old empty-queue quarantine boundary.
    pub(crate) fn finish(mut self, activation_is_quiescent: bool) {
        self.finished = true;
        finish_async_box_release_scope(self.depth, activation_is_quiescent);
    }
}

impl Drop for AsyncBoxReleaseScope {
    fn drop(&mut self) {
        if !self.finished {
            // A Rust unwind must never make a partly released activation's
            // cells reusable. Falling back to the old global quarantine is
            // conservative and keeps the terminal sentinels intact.
            finish_async_box_release_scope(self.depth, false);
        }
    }
}

fn finish_async_box_release_scope(depth: usize, activation_is_quiescent: bool) {
    let mut batch = ASYNC_BOX_RELEASE_SCOPES.with(|scopes| {
        let mut scopes = scopes.borrow_mut();
        assert_eq!(
            scopes.len(),
            depth,
            "async box release scopes must finish in stack order"
        );
        scopes.pop().expect("async box release scope disappeared")
    });
    if !batch.is_empty() {
        if activation_is_quiescent {
            BOX_ACTIVATION_PUBLISH_COUNT.fetch_add(1, Ordering::Relaxed);
            publish_released_batch(&mut batch);
        } else {
            BOX_RELEASE_QUARANTINE.with(|q| q.borrow_mut().append(&mut batch.boxes));
            I32_BOX_RELEASE_QUARANTINE.with(|q| q.borrow_mut().append(&mut batch.i32_boxes));
            BOOL_BOX_RELEASE_QUARANTINE.with(|q| q.borrow_mut().append(&mut batch.bool_boxes));
        }
    }
    debug_assert!(batch.is_empty());
    ASYNC_BOX_RELEASE_BATCH_POOL.with(|pool| pool.borrow_mut().push(batch));
}

fn park_released_box(kind: ReleasedBoxKind, addr: usize) {
    let parked_in_scope = ASYNC_BOX_RELEASE_SCOPES.with(|scopes| {
        let mut scopes = scopes.borrow_mut();
        let Some(batch) = scopes.last_mut() else {
            return false;
        };
        match kind {
            ReleasedBoxKind::Box => batch.boxes.push(addr),
            ReleasedBoxKind::I32 => batch.i32_boxes.push(addr),
            ReleasedBoxKind::Bool => batch.bool_boxes.push(addr),
        }
        true
    });
    if parked_in_scope {
        return;
    }
    match kind {
        ReleasedBoxKind::Box => BOX_RELEASE_QUARANTINE.with(|q| q.borrow_mut().push(addr)),
        ReleasedBoxKind::I32 => I32_BOX_RELEASE_QUARANTINE.with(|q| q.borrow_mut().push(addr)),
        ReleasedBoxKind::Bool => BOOL_BOX_RELEASE_QUARANTINE.with(|q| q.borrow_mut().push(addr)),
    }
}

fn publish_released_batch(batch: &mut ReleasedBoxBatch) {
    publish_released_addresses(&mut batch.boxes, &BOX_FREE_HEAD);
    publish_released_addresses(&mut batch.i32_boxes, &I32_BOX_FREE_HEAD);
    publish_released_addresses(&mut batch.bool_boxes, &BOOL_BOX_FREE_HEAD);
}

fn publish_released_addresses(addresses: &mut Vec<usize>, head: &'static BoxFreeHead) {
    if addresses.is_empty() {
        return;
    }
    BOX_FLUSH_PUBLISHED.fetch_add(addresses.len() as u64, Ordering::Relaxed);
    head.with(|h| {
        let mut next = h.get();
        for addr in addresses.drain(..) {
            // Publishing is the first moment the parked terminal value is
            // dead, so the cell's own bytes can become the free-list link.
            debug_assert_eq!(addr % ALIGN_OF_BOX_CELL, 0);
            unsafe { (addr as *mut usize).write(next) };
            next = addr;
        }
        h.set(next);
    });
}

/// Drain the release quarantines into the free pools. Called at the
/// outermost microtask-pump exit once TASK_QUEUE is empty (see the
/// QUARANTINE doc above for why that boundary), and by tests.
pub fn flush_released_boxes() {
    BOX_FLUSH_COUNT.fetch_add(1, Ordering::Relaxed);
    for (q, head) in [
        (&BOX_RELEASE_QUARANTINE, &BOX_FREE_HEAD),
        (&I32_BOX_RELEASE_QUARANTINE, &I32_BOX_FREE_HEAD),
        (&BOOL_BOX_RELEASE_QUARANTINE, &BOOL_BOX_FREE_HEAD),
    ] {
        q.with(|q| {
            let mut q = q.borrow_mut();
            publish_released_addresses(&mut q, head);
            // Deliberately NOT shrunk. The quarantine is refilled to roughly
            // the same size every interval, so handing the buffer back here
            // just makes the next interval re-grow it: measured, shrinking to
            // 1 KiB each flush cost +5.3 MB peak RSS at BATCHES=1200 in
            // allocator churn, which is the opposite of the point.
        });
    }
}

/// Every box cell is exactly one pointer wide, which is what lets the free
/// list live inside the cells. Asserted rather than assumed: a field added to
/// any box struct would silently make the link write out of bounds.
const ALIGN_OF_BOX_CELL: usize = std::mem::align_of::<Box>();
const _: () = {
    assert!(std::mem::size_of::<Box>() == std::mem::size_of::<usize>());
    assert!(std::mem::size_of::<I32Box>() == std::mem::size_of::<usize>());
    assert!(std::mem::size_of::<BoolBox>() == std::mem::size_of::<usize>());
    assert!(std::mem::align_of::<Box>() >= std::mem::align_of::<usize>());
    assert!(std::mem::align_of::<I32Box>() >= std::mem::align_of::<usize>());
    assert!(std::mem::align_of::<BoolBox>() >= std::mem::align_of::<usize>());
};

/// Pop a cell from an intrusive free list, or 0 when it is empty.
#[inline(always)]
fn pop_free_cell(head: &'static crate::tls_hot::HotKey<std::cell::Cell<usize>>) -> usize {
    head.with(|h| {
        let addr = h.get();
        if addr != 0 {
            // SAFETY: `addr` was minted by `js_*box_alloc*`, is cell-sized and
            // cell-aligned, and its memory is never returned to the allocator,
            // so the link written at publish time is still there.
            h.set(unsafe { (addr as *const usize).read() });
        }
        addr
    })
}

/// Boxes are 8-byte allocations, so bits 0..3 of an address carry no
/// information; index on the bits above them.
#[inline(always)]
fn box_ptr_cache_index(addr: usize) -> usize {
    (addr >> 3) & (BOX_PTR_CACHE_SLOTS - 1)
}

#[inline(always)]
fn box_ptr_cache_hit(cache: &'static BoxPtrCache, addr: usize) -> bool {
    cache.with(|slots| slots[box_ptr_cache_index(addr)].get() == addr)
}

#[inline(always)]
fn box_ptr_cache_record(cache: &'static BoxPtrCache, addr: usize) {
    cache.with(|slots| slots[box_ptr_cache_index(addr)].set(addr));
}

/// Evict `addr` from its direct-mapped cache slot if it currently occupies
/// it. Called on release so a parked cell is invisible to the positive cache
/// too — the parked-cell inertness argument in the QUARANTINE doc relies on
/// every `js_box_get`/`js_box_set` on a parked address falling through to
/// the registry probe and missing.
#[inline(always)]
fn box_ptr_cache_evict(cache: &'static BoxPtrCache, addr: usize) {
    cache.with(|slots| {
        let slot = &slots[box_ptr_cache_index(addr)];
        if slot.get() == addr {
            slot.set(0);
        }
    });
}

/// Allocate a new box with an initial JSValue bit pattern.
#[no_mangle]
pub extern "C" fn js_box_alloc_bits(initial_bits: i64) -> *mut Box {
    BOX_ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
    // #7933 follow-up: serve from the free pool first. A pooled address was
    // minted by this function, cleared and de-registered at release, and is
    // provably unreferenced (see the QUARANTINE doc) — re-registering it
    // with a fresh value is indistinguishable from a fresh allocation.
    let pooled = pop_free_cell(&BOX_FREE_HEAD);
    if pooled != 0 {
        let addr = pooled;
        BOX_POOL_REUSE_COUNT.fetch_add(1, Ordering::Relaxed);
        let ptr = addr as *mut Box;
        unsafe {
            (*ptr).value = initial_bits as u64;
        }
        BOX_REGISTRY.with(|r| {
            r.borrow_mut().insert(addr);
        });
        box_ptr_cache_record(&BOX_PTR_CACHE, addr);
        return ptr;
    }
    unsafe {
        let layout = Layout::new::<Box>();
        let ptr = alloc(layout) as *mut Box;
        if ptr.is_null() {
            // perry#924: oom is rare enough that operators see the
            // downstream crash and react to that; keep the diagnostic
            // available under `PERRY_DEBUG=1` for bisection.
            if std::env::var_os("PERRY_DEBUG").is_some() {
                eprintln!("[PERRY WARN] js_box_alloc: allocation failed — returning null");
            }
            return std::ptr::null_mut();
        }
        (*ptr).value = initial_bits as u64;
        BOX_REGISTRY.with(|r| {
            r.borrow_mut().insert(ptr as usize);
        });
        box_ptr_cache_record(&BOX_PTR_CACHE, ptr as usize);
        ptr
    }
}

/// Compatibility wrapper for legacy f64-lowered boxed locals.
#[no_mangle]
pub extern "C" fn js_box_alloc(initial_value: f64) -> *mut Box {
    js_box_alloc_bits(initial_value.to_bits() as i64)
}

#[no_mangle]
pub extern "C" fn js_i32_box_alloc(initial_value: i32) -> *mut I32Box {
    BOX_ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
    let pooled = pop_free_cell(&I32_BOX_FREE_HEAD);
    if pooled != 0 {
        let addr = pooled;
        BOX_POOL_REUSE_COUNT.fetch_add(1, Ordering::Relaxed);
        let ptr = addr as *mut I32Box;
        unsafe {
            (*ptr).value = initial_value;
        }
        I32_BOX_REGISTRY.with(|r| {
            r.borrow_mut().insert(addr);
        });
        box_ptr_cache_record(&I32_BOX_PTR_CACHE, addr);
        return ptr;
    }
    unsafe {
        let layout = Layout::new::<I32Box>();
        let ptr = alloc(layout) as *mut I32Box;
        if ptr.is_null() {
            if std::env::var_os("PERRY_DEBUG").is_some() {
                eprintln!("[PERRY WARN] js_i32_box_alloc: allocation failed — returning null");
            }
            return std::ptr::null_mut();
        }
        (*ptr).value = initial_value;
        I32_BOX_REGISTRY.with(|r| {
            r.borrow_mut().insert(ptr as usize);
        });
        box_ptr_cache_record(&I32_BOX_PTR_CACHE, ptr as usize);
        ptr
    }
}

#[no_mangle]
pub extern "C" fn js_bool_box_alloc(initial_value: i32) -> *mut BoolBox {
    BOX_ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
    let pooled = pop_free_cell(&BOOL_BOX_FREE_HEAD);
    if pooled != 0 {
        let addr = pooled;
        BOX_POOL_REUSE_COUNT.fetch_add(1, Ordering::Relaxed);
        let ptr = addr as *mut BoolBox;
        unsafe {
            (*ptr).value = initial_value != 0;
        }
        BOOL_BOX_REGISTRY.with(|r| {
            r.borrow_mut().insert(addr);
        });
        box_ptr_cache_record(&BOOL_BOX_PTR_CACHE, addr);
        return ptr;
    }
    unsafe {
        let layout = Layout::new::<BoolBox>();
        let ptr = alloc(layout) as *mut BoolBox;
        if ptr.is_null() {
            if std::env::var_os("PERRY_DEBUG").is_some() {
                eprintln!("[PERRY WARN] js_bool_box_alloc: allocation failed — returning null");
            }
            return std::ptr::null_mut();
        }
        (*ptr).value = initial_value != 0;
        BOOL_BOX_REGISTRY.with(|r| {
            r.borrow_mut().insert(ptr as usize);
        });
        box_ptr_cache_record(&BOOL_BOX_PTR_CACHE, ptr as usize);
        ptr
    }
}

/// #7933 follow-up: release one JSValue box cell at a plain-async
/// activation's terminal state.
///
/// Emitted by codegen for `Stmt::ReleaseBoxes` — ONLY for cells the
/// transform's escape analysis proved no closure can observe
/// (`perry-transform/src/generator/box_release.rs`), which is the same
/// precondition the pre-existing clear-to-`undefined` release relied on.
/// Clears the cell, removes it from the registry, evicts the positive-cache
/// slot, and parks the address in the release quarantine for reuse after the
/// next microtask-drain boundary (see the QUARANTINE doc).
///
/// Idempotent and foreign-pointer-safe by the same gate: a pointer that is
/// not currently registered — already released, never a box, or a
/// perry#4898-style bogus address — is left untouched. The terminal arms of
/// the step machine can re-run on a stray duplicate resume, so double
/// release MUST be a total no-op (a second park of the same address would
/// alias two future activations onto one cell).
#[no_mangle]
pub extern "C" fn js_box_release(ptr: *mut Box) {
    let addr = ptr as usize;
    if !is_plausible_box_ptr(ptr) {
        return;
    }
    let was_registered = BOX_REGISTRY.with(|r| r.borrow_mut().remove(&addr));
    if !was_registered {
        return;
    }
    box_ptr_cache_evict(&BOX_PTR_CACHE, addr);
    unsafe {
        // Cleared BEFORE parking: a parked cell must read as `undefined`
        // through any stale path, and must retain nothing for the GC (the
        // root scanner only walks the registry, which no longer has it).
        (*ptr).value = crate::value::TAG_UNDEFINED;
    }
    park_released_box(ReleasedBoxKind::Box, addr);
    BOX_RELEASE_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// `js_box_release` for the compiler-private i32 control cells
/// (`__gen_state` / `__gen_pending_type`). Same contract as
/// `js_box_release`, with one twist: generated async-step code reads these
/// cells with RAW inline loads (`load_async_i32_control_cell`), never
/// through the registry-checked getter — so the PARKED VALUE is what a stray
/// duplicate resume would observe. Park `-1`: the linearizer numbers states
/// from 0, so `-1` matches no dispatch case and no catch-route condition,
/// and the dispatch loop's default arm returns the done iter-result.
/// (Unreachable in practice anyway: the parked `__gen_done = true` below
/// short-circuits a stray resume before any state read.)
#[no_mangle]
pub extern "C" fn js_i32_box_release(ptr: *mut I32Box) {
    let addr = ptr as usize;
    if !is_plausible_box_ptr(ptr.cast::<Box>()) {
        return;
    }
    let was_registered = I32_BOX_REGISTRY.with(|r| r.borrow_mut().remove(&addr));
    if !was_registered {
        return;
    }
    box_ptr_cache_evict(&I32_BOX_PTR_CACHE, addr);
    unsafe {
        (*ptr).value = -1;
    }
    park_released_box(ReleasedBoxKind::I32, addr);
    BOX_RELEASE_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// `js_box_release` for the compiler-private i1 control cells
/// (`__gen_done` / `__gen_executing`). Same contract. Like the i32 cells,
/// generated code reads these with RAW inline loads, so the parked value is
/// what a stray duplicate resume observes. Park `true`: a stray resume's
/// first control read is `if (__gen_done) return {done: true}` — parked
/// `true` takes byte-for-byte the pre-release terminal short-circuit
/// (`__gen_done` really was `true` when the activation completed).
/// `__gen_executing` also parks `true`, which is fine: the executing guard
/// belongs to the user-callable generator `.next()` wrappers, and
/// generators never release (only `was_plain_async` activations do, and
/// their fused step body never reads `__gen_executing`).
#[no_mangle]
pub extern "C" fn js_bool_box_release(ptr: *mut BoolBox) {
    let addr = ptr as usize;
    if !is_plausible_box_ptr(ptr.cast::<Box>()) {
        return;
    }
    let was_registered = BOOL_BOX_REGISTRY.with(|r| r.borrow_mut().remove(&addr));
    if !was_registered {
        return;
    }
    box_ptr_cache_evict(&BOOL_BOX_PTR_CACHE, addr);
    unsafe {
        (*ptr).value = true;
    }
    park_released_box(ReleasedBoxKind::Bool, addr);
    BOX_RELEASE_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// GC root scanner: walk every registered box and `mark` the JSValue bit
/// value inside. Heap pointers stored inside boxes (e.g. the generator
/// state machine's iter object held in a mutable-capture box) must be
/// kept alive across collections. The box pointer itself is _not_ a
/// heap value the runtime tracks — `BOX_REGISTRY` is the source of
/// truth for "every live box right now" — so we use the standard root
/// scanner protocol: dispatch every stored JSValue bit pattern to `mark`
/// and let the GC trace into it.
pub fn scan_box_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_box_roots_mut(&mut visitor);
}

pub fn scan_box_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    BOX_REGISTRY.with(|r| {
        let r = r.borrow();
        for &addr in r.iter() {
            let ptr = addr as *mut Box;
            // Defensive: the registry should only contain valid live
            // pointers, but if a stale entry slipped through we'd
            // segfault on the deref. The tight bounds check on the
            // address (alloc gives 8-aligned pointers in user space)
            // matches `is_plausible_box_ptr` to keep this a no-op for
            // any pathological entry.
            if addr >= 0x1000 && (addr as u64) < 0x0001_0000_0000_0000 && addr % 8 == 0 {
                unsafe {
                    visitor.visit_nanbox_u64_raw_slot(&raw mut (*ptr).value);
                }
            }
        }
    });
}

/// Get the raw JSValue bit pattern from a box.
///
/// Same robustness as `js_box_set`: invalid pointers return `undefined`
/// rather than dereferencing. See perry#393 for the failure mode.
#[no_mangle]
pub extern "C" fn js_box_get_bits(ptr: *mut Box) -> i64 {
    unsafe {
        if !is_registered_box_ptr(ptr) {
            // perry#924: production services see these in tight bursts of
            // 3 synced with normal request handling and the operator can't
            // tell whether anything is wrong. The path is correctness-safe
            // (we already return a defined value to the caller); gate the
            // diagnostic behind `PERRY_DEBUG=1` so it only surfaces during
            // bisection.
            if std::env::var_os("PERRY_DEBUG").is_some() {
                let count = BOX_GET_NULL_COUNT.fetch_add(1, Ordering::Relaxed);
                if count < 3 {
                    eprintln!(
                        "[PERRY WARN] js_box_get: invalid box pointer {:p} #{}",
                        ptr, count
                    );
                }
            }
            // perry#4926: with codegen entry-initializing boxed slots to
            // TAG_UNDEFINED, this arm is the read-before-initialization
            // path for a boxed variable — in JS that reads as `undefined`
            // (Perry has no TDZ), not as the number NaN. TAG_UNDEFINED is
            // itself a quiet-NaN bit pattern, so numeric consumers behave
            // exactly as before; JS-level checks (`typeof`, `== null`)
            // now see `undefined`.
            return crate::value::TAG_UNDEFINED as i64;
        }
        let bits = (*ptr).value;
        // Temporal Dead Zone: a lexical `let`/`const`/`class` box seeded with
        // the TAG_TDZ sentinel at scope entry throws a spec ReferenceError when
        // read before its declaration runs (which overwrites the sentinel with
        // a real value). TAG_TDZ is a reserved bit pattern no legitimate value
        // ever holds, so this branch is only ever taken on a genuine
        // read-before-initialization — making the check zero-regression for
        // every already-initialized box. The name is passed as `undefined`
        // because this choke point is name-agnostic (it serves direct,
        // closure-captured, and compound reads alike); the resulting message is
        // the spec-generic form.
        if bits == crate::value::TAG_TDZ {
            // #6044 regression (#6052): Perry-internal materialization reads —
            // the class-capture decl-site snapshot refreshes emitted after EACH
            // captured var's assignment (`RegisterClassCaptures`, the #6037
            // refresh strategy) — legally observe sibling captures that are
            // still in their dead zone (`const _fs = ..; <refresh reads _path>;
            // const _path = ..`, the SWC CJS interop shape). Those are not user
            // reads: pre-TDZ they snapshotted `undefined` and the next refresh
            // fixed the value up. Inside the codegen-bracketed suppression
            // window, keep exactly that behavior instead of throwing.
            if TDZ_SUPPRESS_DEPTH.with(|d| d.get()) > 0 {
                return crate::value::TAG_UNDEFINED as i64;
            }
            crate::error::js_throw_reference_error_tdz(f64::from_bits(crate::value::TAG_UNDEFINED));
        }
        bits as i64
    }
}

crate::perry_thread_local! {
    /// #6052: >0 while codegen-emitted Perry-internal materialization reads
    /// (the `RegisterClassCaptures` decl-site snapshot refresh) are running —
    /// a dead-zone box then reads as `undefined` (pre-#6044 behavior) instead
    /// of throwing. Never spans user code: the bracketed window contains only
    /// side-effect-free capture loads.
    static TDZ_SUPPRESS_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Enter a TDZ-suppression window (see `TDZ_SUPPRESS_DEPTH`). Emitted by
/// codegen immediately before a `RegisterClassCaptures` snapshot's capture
/// loads; paired with `js_tdz_suppress_end`.
#[no_mangle]
pub extern "C" fn js_tdz_suppress_begin() {
    TDZ_SUPPRESS_DEPTH.with(|d| d.set(d.get().saturating_add(1)));
}

/// Leave the TDZ-suppression window opened by `js_tdz_suppress_begin`.
#[no_mangle]
pub extern "C" fn js_tdz_suppress_end() {
    TDZ_SUPPRESS_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
}

/// Keepalive anchors for the auto-optimize whole-program build (generated-code-
/// only callees — without these the symbols dead-strip and the app link fails).
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_TDZ_SUPPRESS_BEGIN: extern "C" fn() = js_tdz_suppress_begin;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_TDZ_SUPPRESS_END: extern "C" fn() = js_tdz_suppress_end;

/// Compatibility wrapper for legacy f64-lowered boxed locals.
#[no_mangle]
pub extern "C" fn js_box_get(ptr: *mut Box) -> f64 {
    f64::from_bits(js_box_get_bits(ptr) as u64)
}

#[no_mangle]
pub extern "C" fn js_i32_box_get(ptr: *mut I32Box) -> i32 {
    unsafe {
        if !is_registered_i32_box_ptr(ptr) {
            if std::env::var_os("PERRY_DEBUG").is_some() {
                let count = I32_BOX_GET_NULL_COUNT.fetch_add(1, Ordering::Relaxed);
                if count < 3 {
                    eprintln!(
                        "[PERRY WARN] js_i32_box_get: invalid box pointer {:p} #{}",
                        ptr, count
                    );
                }
            }
            return 0;
        }
        (*ptr).value
    }
}

#[no_mangle]
pub extern "C" fn js_bool_box_get(ptr: *mut BoolBox) -> i32 {
    unsafe {
        if !is_registered_bool_box_ptr(ptr) {
            if std::env::var_os("PERRY_DEBUG").is_some() {
                let count = BOOL_BOX_GET_NULL_COUNT.fetch_add(1, Ordering::Relaxed);
                if count < 3 {
                    eprintln!(
                        "[PERRY WARN] js_bool_box_get: invalid box pointer {:p} #{}",
                        ptr, count
                    );
                }
            }
            return 0;
        }
        i32::from((*ptr).value)
    }
}

/// Set the raw JSValue bit pattern in a box.
///
/// Robust against bogus pointers: in addition to the null check, we
/// reject obviously-invalid pointers (below the first user page or
/// above the 48-bit user-address ceiling) and pointers that aren't
/// 8-byte aligned. This avoids SIGSEGV on `(*ptr).value = value` when
/// upstream codegen hands us a stale/uninitialized slot — a known
/// failure mode for closure prologues at hub-scale (perry#393).
/// Boxes are heap-allocated 8-byte JSValue bit slots; a non-aligned or low/high
/// pointer is definitely wrong, so a silent skip + telemetry warning
/// is strictly safer than dereferencing it.
#[no_mangle]
pub extern "C" fn js_box_set_bits(ptr: *mut Box, value_bits: i64) {
    unsafe {
        if !is_registered_box_ptr(ptr) {
            // perry#924: silent-skip is correctness-safe (caller's box
            // mutation is dropped, which is the same as no closure
            // capture having existed). Gate diagnostics behind
            // `PERRY_DEBUG=1` to keep production stderr clean.
            if std::env::var_os("PERRY_DEBUG").is_some() {
                let count = BOX_SET_NULL_COUNT.fetch_add(1, Ordering::Relaxed);
                if count < 3 {
                    eprintln!(
                        "[PERRY WARN] js_box_set: invalid box pointer {:p} #{} (value bits: 0x{:016x})",
                        ptr,
                        count,
                        value_bits as u64
                    );
                }
            }
            return;
        }
        let bits = value_bits as u64;
        (*ptr).value = bits;
        crate::gc::runtime_write_barrier_root_nanbox(bits);
    }
}

/// Compatibility wrapper for legacy f64-lowered boxed locals.
#[no_mangle]
pub extern "C" fn js_box_set(ptr: *mut Box, value: f64) {
    js_box_set_bits(ptr, value.to_bits() as i64);
}

#[no_mangle]
pub extern "C" fn js_i32_box_set(ptr: *mut I32Box, value: i32) {
    unsafe {
        if !is_registered_i32_box_ptr(ptr) {
            if std::env::var_os("PERRY_DEBUG").is_some() {
                let count = I32_BOX_SET_NULL_COUNT.fetch_add(1, Ordering::Relaxed);
                if count < 3 {
                    eprintln!(
                        "[PERRY WARN] js_i32_box_set: invalid box pointer {:p} #{} (value: {})",
                        ptr, count, value
                    );
                }
            }
            return;
        }
        (*ptr).value = value;
    }
}

#[no_mangle]
pub extern "C" fn js_bool_box_set(ptr: *mut BoolBox, value: i32) {
    unsafe {
        if !is_registered_bool_box_ptr(ptr) {
            if std::env::var_os("PERRY_DEBUG").is_some() {
                let count = BOOL_BOX_SET_NULL_COUNT.fetch_add(1, Ordering::Relaxed);
                if count < 3 {
                    eprintln!(
                        "[PERRY WARN] js_bool_box_set: invalid box pointer {:p} #{} (value: {})",
                        ptr, count, value
                    );
                }
            }
            return;
        }
        (*ptr).value = value != 0;
    }
}

/// Cheap pointer-sanity test — same threat model as `get_valid_func_ptr`
/// in closure.rs, adapted for box-shaped allocations.
///
/// A `*mut Box` from `js_box_alloc` is a Rust-`alloc()` heap pointer,
/// which on x86_64 Linux/macOS lives in the 47-bit user-address half
/// of the address space and (because `Layout::new::<Box>()` yields
/// `align = 8`) is 8-byte aligned. Pointers below the first user page
/// or above the user-address ceiling, or unaligned ones, can only come
/// from stale/uninitialized stack slots reinterpreted as box pointers.
///
/// perry#4898: the structural checks are necessary but **not sufficient**.
/// A miscompiled `js_box_set` can be handed a box-pointer operand that was
/// effectively `undef`/poison at the IR level (e.g. a mutable-capture box
/// whose allocation was elided on the taken path). LLVM then fills the
/// register with whatever was conveniently live — under typed-feedback
/// (#854) instrumentation that is the read-only `..._guard` string constant
/// passed to `js_typed_feedback_register_site`. That constant is ≥0x1000,
/// untagged (top-16 zero), and 8-byte aligned, so it sails through every
/// structural check — and `(*ptr).value = value` then writes into
/// `__TEXT.__cstring`, a SIGBUS. The address `read_static`-looks like a box
/// but isn't one. `is_registered_box_ptr` closes that gap: a pointer that
/// `js_box_alloc` never minted is rejected before the deref.
#[inline]
fn is_plausible_box_ptr(ptr: *mut Box) -> bool {
    let addr = ptr as usize;
    if addr == 0 {
        return false;
    }
    if addr < 0x1000 {
        return false;
    }
    if (addr as u64) >= 0x0001_0000_0000_0000 {
        return false;
    }
    if !addr.is_multiple_of(std::mem::align_of::<Box>()) {
        return false;
    }
    true
}

/// Authoritative box-pointer check: the address must have been minted by
/// `js_box_alloc` and be currently registered. Box-cell memory is never
/// returned to the allocator — a cell is live-registered or parked in the
/// release pool (#7933 follow-up), never recycled into a non-box
/// allocation — so membership has no false negatives for a live box and no
/// stale-reuse hazard: an address that isn't in the registry is either not
/// a box at all or a released (inert, `undefined`-reading) cell, and
/// treating both as "not a box" is exactly right. This is what stops a
/// stray read-only/garbage pointer (perry#4898) from being dereferenced as
/// a box, and what makes a parked cell's reads/writes inert.
#[inline]
fn is_registered_box_ptr(ptr: *mut Box) -> bool {
    if !is_plausible_box_ptr(ptr) {
        return false;
    }
    let addr = ptr as usize;
    if box_ptr_cache_hit(&BOX_PTR_CACHE, addr) {
        return true;
    }
    let present = BOX_REGISTRY.with(|r| r.borrow().contains(&addr));
    if present {
        box_ptr_cache_record(&BOX_PTR_CACHE, addr);
    }
    present
}

/// If `slot_bits` (the raw contents of a closure capture slot) is a registered
/// box pointer, return the JSValue bits stored *inside* that box; otherwise
/// return `None`.
///
/// A closure that captures a boxed local — every body local of an `async`
/// function (the async-to-generator transform boxes them all), plus any
/// mutable capture — stores the raw box pointer in its capture slot rather
/// than a NaN-boxed value (see the codegen closure lowering in
/// `perry-codegen/src/expr/closure.rs`). That pointer addresses a box in the
/// *current thread's* thread-local `BOX_REGISTRY`, so it is
/// meaningless on any other thread. The `perry/thread` serializer uses this to
/// unwrap such a slot to the value the box actually holds before deep-copying
/// it across the boundary (#6520 — without it the worker read the captured
/// value as `undefined`/empty).
///
/// Registry membership is authoritative: any NaN-boxed value or real double
/// has its high bits set and fails `is_plausible_box_ptr`, so this only ever
/// matches a genuine live box pointer, never a coincidental capture value.
#[inline]
pub fn box_slot_contents_bits(slot_bits: u64) -> Option<u64> {
    let ptr = slot_bits as usize as *mut Box;
    if is_registered_box_ptr(ptr) {
        // Safety: the address is in BOX_REGISTRY, so it was minted by
        // `js_box_alloc` and points at a live `Box` (cell memory is never
        // returned to the allocator; see the release-pool doc).
        Some(unsafe { (*ptr).value })
    } else {
        None
    }
}

#[inline]
fn is_registered_i32_box_ptr(ptr: *mut I32Box) -> bool {
    if !is_plausible_box_ptr(ptr.cast::<Box>()) {
        return false;
    }
    let addr = ptr as usize;
    if box_ptr_cache_hit(&I32_BOX_PTR_CACHE, addr) {
        return true;
    }
    let present = I32_BOX_REGISTRY.with(|r| r.borrow().contains(&addr));
    if present {
        box_ptr_cache_record(&I32_BOX_PTR_CACHE, addr);
    }
    present
}

#[inline]
fn is_registered_bool_box_ptr(ptr: *mut BoolBox) -> bool {
    if !is_plausible_box_ptr(ptr.cast::<Box>()) {
        return false;
    }
    let addr = ptr as usize;
    if box_ptr_cache_hit(&BOOL_BOX_PTR_CACHE, addr) {
        return true;
    }
    let present = BOOL_BOX_REGISTRY.with(|r| r.borrow().contains(&addr));
    if present {
        box_ptr_cache_record(&BOOL_BOX_PTR_CACHE, addr);
    }
    present
}

#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_BOX_ALLOC_BITS: extern "C" fn(i64) -> *mut Box = js_box_alloc_bits;

#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_BOX_RELEASE: extern "C" fn(*mut Box) = js_box_release;

#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_I32_BOX_RELEASE: extern "C" fn(*mut I32Box) = js_i32_box_release;

#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_BOOL_BOX_RELEASE: extern "C" fn(*mut BoolBox) = js_bool_box_release;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_BOX_GET_BITS: extern "C" fn(*mut Box) -> i64 = js_box_get_bits;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_BOX_SET_BITS: extern "C" fn(*mut Box, i64) = js_box_set_bits;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_BOX_ALLOC: extern "C" fn(f64) -> *mut Box = js_box_alloc;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_BOX_GET: extern "C" fn(*mut Box) -> f64 = js_box_get;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_BOX_SET: extern "C" fn(*mut Box, f64) = js_box_set;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_I32_BOX_ALLOC: extern "C" fn(i32) -> *mut I32Box = js_i32_box_alloc;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_I32_BOX_GET: extern "C" fn(*mut I32Box) -> i32 = js_i32_box_get;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_I32_BOX_SET: extern "C" fn(*mut I32Box, i32) = js_i32_box_set;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_BOOL_BOX_ALLOC: extern "C" fn(i32) -> *mut BoolBox = js_bool_box_alloc;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_BOOL_BOX_GET: extern "C" fn(*mut BoolBox) -> i32 = js_bool_box_get;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_BOOL_BOX_SET: extern "C" fn(*mut BoolBox, i32) = js_bool_box_set;

#[cfg(test)]
pub(crate) fn test_clear_box_registry() {
    BOX_REGISTRY.with(|r| r.borrow_mut().clear());
    I32_BOX_REGISTRY.with(|r| r.borrow_mut().clear());
    BOOL_BOX_REGISTRY.with(|r| r.borrow_mut().clear());
    BOX_FREE_HEAD.with(|h| h.set(0));
    I32_BOX_FREE_HEAD.with(|h| h.set(0));
    BOOL_BOX_FREE_HEAD.with(|h| h.set(0));
    BOX_RELEASE_QUARANTINE.with(|q| q.borrow_mut().clear());
    I32_BOX_RELEASE_QUARANTINE.with(|q| q.borrow_mut().clear());
    BOOL_BOX_RELEASE_QUARANTINE.with(|q| q.borrow_mut().clear());
    ASYNC_BOX_RELEASE_SCOPES.with(|scopes| scopes.borrow_mut().clear());
    ASYNC_BOX_RELEASE_BATCH_POOL.with(|pool| pool.borrow_mut().clear());
    // Registry membership is not monotonic any more (#8208: `js_*box_release`
    // de-registers a completed activation's cells), so the positive cache is
    // kept coherent by an eviction on every un-registration rather than by
    // never un-registering. This wholesale clear is the bulk case — it exists
    // only for tests — and it must drop the caches for the same reason a single
    // release evicts one slot: otherwise a later test would see a stale "yes"
    // for an address this call just un-registered.
    for cache in [&BOX_PTR_CACHE, &I32_BOX_PTR_CACHE, &BOOL_BOX_PTR_CACHE] {
        cache.with(|slots| {
            for slot in slots {
                slot.set(0);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// perry#4898: a structurally-plausible pointer that `js_box_alloc`
    /// never minted (here, a `&'static` read-only constant that is ≥0x1000,
    /// untagged, and 8-byte aligned — exactly the shape of the leaked
    /// `..._guard` string) must NOT be dereferenced by `js_box_set`. Before
    /// the registry check this stored into read-only memory → SIGBUS.
    #[test]
    fn box_set_skips_unregistered_plausible_pointer() {
        test_clear_box_registry();
        // 8-byte aligned static — passes every structural check, is not a box.
        static RODATA: [u64; 2] = [0xDEAD_BEEF, 0xFEED_FACE];
        let fake = (&RODATA[0] as *const u64) as *mut Box;
        assert!(is_plausible_box_ptr(fake), "test needs a plausible ptr");
        assert!(!is_registered_box_ptr(fake), "fake must not be registered");
        // Must be a silent no-op, not a write/crash.
        js_box_set(fake, 1.0);
        js_box_set_bits(
            fake,
            crate::value::JSValue::try_short_string(b"bad")
                .unwrap()
                .bits() as i64,
        );
        assert_eq!(RODATA[0], 0xDEAD_BEEF, "rodata must be untouched");
        // Reads from an unregistered pointer return `undefined` (perry#4926:
        // the read-before-initialization value of a boxed variable), never
        // deref. TAG_UNDEFINED is a NaN bit pattern, so this also preserves
        // the older "returns NaN" numeric behavior.
        assert_eq!(
            js_box_get_bits(fake) as u64,
            crate::value::TAG_UNDEFINED,
            "unregistered bits box read must yield undefined"
        );
        assert_eq!(
            js_box_get(fake).to_bits(),
            crate::value::TAG_UNDEFINED,
            "unregistered box read must yield undefined"
        );
    }

    /// A real `js_box_alloc` box still round-trips through set/get after the
    /// registry gate (no false negatives on genuine boxes).
    #[test]
    fn box_set_get_roundtrips_for_real_box() {
        test_clear_box_registry();
        let b = js_box_alloc(3.5);
        assert!(is_registered_box_ptr(b));
        assert_eq!(js_box_get(b), 3.5);
        js_box_set(b, 42.0);
        assert_eq!(js_box_get(b), 42.0);
    }

    /// The bits ABI is the canonical boxed-local storage path for dynamic
    /// JSValues. It must not turn Perry's NaN-boxed non-number values into a
    /// numeric NaN payload.
    #[test]
    fn box_bits_roundtrips_non_number_tags_exactly() {
        test_clear_box_registry();
        let cases = [
            crate::value::JSValue::int32(-17).bits(),
            crate::value::JSValue::try_short_string(b"ok")
                .unwrap()
                .bits(),
            crate::value::TAG_UNDEFINED,
        ];

        for bits in cases {
            let b = js_box_alloc_bits(bits as i64);
            assert!(is_registered_box_ptr(b));
            assert_eq!(js_box_get_bits(b) as u64, bits);
            assert_eq!(js_box_get(b).to_bits(), bits);

            let replacement = crate::value::JSValue::try_short_string(b"next")
                .unwrap()
                .bits();
            js_box_set_bits(b, replacement as i64);
            assert_eq!(js_box_get_bits(b) as u64, replacement);
            assert_eq!(js_box_get(b).to_bits(), replacement);
        }
    }

    #[test]
    fn primitive_control_boxes_round_trip_and_reject_foreign_pointers() {
        test_clear_box_registry();
        let i32_box = js_i32_box_alloc(7);
        assert!(is_registered_i32_box_ptr(i32_box));
        assert_eq!(js_i32_box_get(i32_box), 7);
        js_i32_box_set(i32_box, -3);
        assert_eq!(js_i32_box_get(i32_box), -3);

        let bool_box = js_bool_box_alloc(0);
        assert!(is_registered_bool_box_ptr(bool_box));
        assert_eq!(js_bool_box_get(bool_box), 0);
        js_bool_box_set(bool_box, 1);
        assert_eq!(js_bool_box_get(bool_box), 1);

        let ordinary_box = js_box_alloc(1.0);
        assert_eq!(js_i32_box_get(ordinary_box.cast::<I32Box>()), 0);
        js_i32_box_set(ordinary_box.cast::<I32Box>(), 99);
        assert_eq!(js_box_get(ordinary_box), 1.0);
    }

    /// The direct-mapped positive cache in front of `BOX_REGISTRY` must not
    /// widen what counts as a box. Sabotage shape: warm the cache with a real
    /// box, then probe a plausible-but-unregistered address that lands in the
    /// SAME cache slot. A cache that compared only the slot index (rather than
    /// the full address) would answer "registered" and `js_box_set` would then
    /// write through a pointer perry#4898 exists to reject.
    #[test]
    fn box_ptr_cache_rejects_a_colliding_unregistered_address() {
        test_clear_box_registry();
        let real = js_box_alloc_bits(crate::value::JSValue::int32(5).bits() as i64);
        assert!(
            is_registered_box_ptr(real),
            "warm the cache with a real box"
        );

        // Every 8-byte-aligned address whose (addr >> 3) is congruent mod the
        // slot count collides with `real`. Walk candidates until one is both
        // plausible and unregistered — `real + 8 * SLOTS * k` is guaranteed to
        // collide by construction.
        let real_addr = real as usize;
        let mut collided = 0usize;
        for k in 1..64usize {
            let candidate = real_addr + 8 * BOX_PTR_CACHE_SLOTS * k;
            let candidate_ptr = candidate as *mut Box;
            if !is_plausible_box_ptr(candidate_ptr) {
                continue;
            }
            if BOX_REGISTRY.with(|r| r.borrow().contains(&candidate)) {
                continue;
            }
            assert_eq!(
                box_ptr_cache_index(candidate),
                box_ptr_cache_index(real_addr),
                "candidate must map to the same cache slot"
            );
            assert!(
                !is_registered_box_ptr(candidate_ptr),
                "a colliding unregistered address must still be rejected"
            );
            collided += 1;
            if collided == 4 {
                break;
            }
        }
        assert!(
            collided > 0,
            "no colliding candidate found — test is vacuous"
        );

        // And the real box still reads back correctly after those misses.
        assert_eq!(
            js_box_get_bits(real) as u64,
            crate::value::JSValue::int32(5).bits()
        );
    }

    /// A box evicted from the cache by later allocations is still recognised —
    /// the cache is an accelerator, never the source of truth.
    #[test]
    fn box_ptr_cache_eviction_does_not_lose_a_real_box() {
        test_clear_box_registry();
        let first = js_box_alloc(1.0);
        assert!(is_registered_box_ptr(first));

        // Allocate well past the cache size so `first` is certainly evicted.
        let mut others = Vec::new();
        for i in 0..(BOX_PTR_CACHE_SLOTS * 8) {
            let b = js_box_alloc(i as f64);
            assert!(is_registered_box_ptr(b));
            others.push(b);
        }

        assert!(
            is_registered_box_ptr(first),
            "eviction must fall through to the authoritative registry"
        );
        assert_eq!(js_box_get(first), 1.0);
        js_box_set(first, 9.0);
        assert_eq!(js_box_get(first), 9.0);
    }

    /// The three registries have independent caches: an ordinary box address
    /// must never be accepted as an i32/bool box just because it is cached in
    /// the ordinary registry's table.
    #[test]
    fn box_ptr_caches_do_not_cross_kinds() {
        test_clear_box_registry();
        let ordinary = js_box_alloc(1.0);
        assert!(is_registered_box_ptr(ordinary));
        assert!(!is_registered_i32_box_ptr(ordinary.cast::<I32Box>()));
        assert!(!is_registered_bool_box_ptr(ordinary.cast::<BoolBox>()));

        let i32_box = js_i32_box_alloc(3);
        assert!(is_registered_i32_box_ptr(i32_box));
        assert!(!is_registered_box_ptr(i32_box.cast::<Box>()));
        assert!(!is_registered_bool_box_ptr(i32_box.cast::<BoolBox>()));
    }

    /// #6520: the thread-boundary serializer unwraps a capture slot that holds
    /// a box pointer to the value inside. `box_slot_contents_bits` returns the
    /// contained JSValue bits for a real box and `None` for anything else — a
    /// plain NaN-boxed value (high tag bits set → not a plausible box address),
    /// a plausible-but-unregistered pointer, and a null slot.
    #[test]
    fn box_slot_contents_unwraps_only_registered_boxes() {
        test_clear_box_registry();

        // A real box: returns the bits it holds, not the pointer.
        let inner = crate::value::JSValue::int32(1234).bits();
        let b = js_box_alloc_bits(inner as i64);
        let slot_bits = b as usize as u64; // codegen stores the raw box ptr here
        assert_eq!(box_slot_contents_bits(slot_bits), Some(inner));

        // A NaN-boxed non-box value (its own tag bits are set) is not a box.
        assert_eq!(box_slot_contents_bits(inner), None);
        assert_eq!(box_slot_contents_bits(crate::value::TAG_UNDEFINED), None);

        // A plausible pointer that was never minted as a box.
        static RODATA: [u64; 2] = [0xDEAD_BEEF, 0xFEED_FACE];
        let fake = (&RODATA[0] as *const u64) as usize as u64;
        assert!(is_plausible_box_ptr(fake as usize as *mut Box));
        assert_eq!(box_slot_contents_bits(fake), None);

        // Null / near-null slots.
        assert_eq!(box_slot_contents_bits(0), None);
    }
}

#[cfg(test)]
mod release_tests {
    use super::*;

    /// `BOX_ALLOC_COUNT` / `BOX_POOL_REUSE_COUNT` / `BOX_RELEASE_COUNT` are
    /// process-global atomics, while the registries, quarantines and free
    /// lists they describe are THREAD-LOCAL. Any test that asserts on a
    /// counter *delta* is therefore not isolated by `test_clear_box_registry`
    /// alone — a sibling test allocating on another harness thread lands in
    /// the same atomics and moves the delta under it. Observed exactly that:
    /// these tests pass under `--test-threads=1` and fail in parallel.
    ///
    /// Serialise the counter-asserting tests against each other. Tests that
    /// only assert on addresses and registry membership are thread-local and
    /// need no lock.
    fn counter_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        // A panicking test poisons the lock; the data is `()`, so recovering
        // is right — otherwise one failure cascades into spurious ones.
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A released cell must be INERT: de-registered (reads `undefined`,
    /// writes dropped), evicted from the positive cache, and parked exactly
    /// once no matter how many times the terminal arm re-runs (#7933
    /// follow-up; the stray-duplicate-resume path re-runs the release list).
    #[test]
    fn released_cell_is_inert_and_release_is_idempotent() {
        super::test_clear_box_registry();
        let ptr = js_box_alloc_bits(crate::value::TAG_TRUE as i64);
        assert!(is_registered_box_ptr(ptr));
        js_box_release(ptr);
        assert!(
            !is_registered_box_ptr(ptr),
            "released cell must be de-registered (and cache-evicted)"
        );
        assert_eq!(
            js_box_get_bits(ptr) as u64,
            crate::value::TAG_UNDEFINED,
            "released cell must read undefined"
        );
        js_box_set_bits(ptr, crate::value::TAG_TRUE as i64);
        assert_eq!(
            unsafe { (*ptr).value },
            crate::value::TAG_UNDEFINED,
            "write to a released cell must be dropped"
        );
        // Idempotence: a second release must not double-park the address —
        // a double-park would hand the same cell to two future activations.
        js_box_release(ptr);
        js_box_release(ptr);
        let parked = BOX_RELEASE_QUARANTINE
            .with(|q| q.borrow().iter().filter(|&&a| a == ptr as usize).count());
        assert_eq!(parked, 1, "double release must park exactly once");
    }

    /// Reuse contract: a released cell becomes allocatable only AFTER the
    /// quarantine flush (the outermost microtask-pump boundary in production),
    /// and the reused cell is re-registered with the fresh initial value.
    #[test]
    fn released_cell_is_reused_only_after_flush() {
        super::test_clear_box_registry();
        let first = js_box_alloc_bits(1.0f64.to_bits() as i64);
        js_box_release(first);
        // Not flushed yet: allocation must NOT reuse the parked cell.
        let second = js_box_alloc_bits(2.0f64.to_bits() as i64);
        assert_ne!(
            first as usize, second as usize,
            "quarantined cell must not be reused before the flush boundary"
        );
        flush_released_boxes();
        let third = js_box_alloc_bits(3.0f64.to_bits() as i64);
        assert_eq!(
            first as usize, third as usize,
            "flushed cell must be reused by the next allocation"
        );
        assert!(is_registered_box_ptr(third), "reused cell re-registers");
        assert_eq!(js_box_get_bits(third), 3.0f64.to_bits() as i64);
    }

    /// #8213: the normal terminal path no longer waits for the whole process
    /// turn to drain. The current invocation keeps its cells quarantined while
    /// generated terminal code is still running, then publishes them as soon
    /// as the caller proves this activation has no queued resume.
    #[test]
    fn quiescent_async_activation_publishes_at_step_return() {
        super::test_clear_box_registry();
        let scope = begin_async_box_release_scope();
        let first = js_box_alloc_bits(1.0f64.to_bits() as i64);
        js_box_release(first);

        let during_terminal_path = js_box_alloc_bits(2.0f64.to_bits() as i64);
        assert_ne!(
            first, during_terminal_path,
            "a step must not reuse its own cell before the terminal path returns"
        );

        scope.finish(true);
        let after_step_return = js_box_alloc_bits(3.0f64.to_bits() as i64);
        assert_eq!(
            first, after_step_return,
            "a quiescent activation should publish without a global queue drain"
        );
    }

    /// A queued duplicate resume keeps the pre-#8213 safety boundary: the
    /// parked terminal value remains intact until the outer empty-queue flush.
    #[test]
    fn non_quiescent_async_activation_falls_back_to_global_quarantine() {
        super::test_clear_box_registry();
        let scope = begin_async_box_release_scope();
        let first = js_bool_box_alloc(0);
        js_bool_box_release(first);
        scope.finish(false);

        assert!(
            unsafe { (*first).value },
            "a queued duplicate resume must retain the terminal sentinel"
        );
        let before_empty_queue = js_bool_box_alloc(0);
        assert_ne!(first, before_empty_queue);

        flush_released_boxes();
        let after_empty_queue = js_bool_box_alloc(0);
        assert_eq!(first, after_empty_queue);
    }

    /// Nested async entry must not merge release ownership: the inner step may
    /// become reusable while the synchronously suspended outer invocation is
    /// still running and its own cell remains quarantined.
    #[test]
    fn nested_async_release_scopes_finish_independently() {
        super::test_clear_box_registry();
        let outer = begin_async_box_release_scope();
        let outer_cell = js_box_alloc_bits(1.0f64.to_bits() as i64);
        js_box_release(outer_cell);

        let inner = begin_async_box_release_scope();
        let inner_cell = js_box_alloc_bits(2.0f64.to_bits() as i64);
        js_box_release(inner_cell);
        inner.finish(true);

        let reused_inner = js_box_alloc_bits(3.0f64.to_bits() as i64);
        assert_eq!(inner_cell, reused_inner);
        assert_ne!(outer_cell, reused_inner);

        outer.finish(true);
        let reused_outer = js_box_alloc_bits(4.0f64.to_bits() as i64);
        assert_eq!(outer_cell, reused_outer);
    }

    /// Generated async-step code reads the compiler-private control cells
    /// with RAW loads (`load_async_i32_control_cell` /
    /// `load_async_i1_control_cell`), never through the registry-checked
    /// getters — so the PARKED VALUES are load-bearing: a stray duplicate
    /// resume must observe `__gen_done == true` (the terminal short-circuit)
    /// and, were it ever to read state, `-1` (no dispatch case matches).
    #[test]
    fn typed_control_cells_park_terminal_values() {
        super::test_clear_box_registry();
        let state = js_i32_box_alloc(7);
        let done = js_bool_box_alloc(0);
        js_i32_box_release(state);
        js_bool_box_release(done);
        assert_eq!(
            unsafe { (*state).value },
            -1,
            "parked i32 control cell must raw-read as -1 (no state)"
        );
        assert!(
            unsafe { (*done).value },
            "parked i1 control cell must raw-read as true (done)"
        );
        // And the checked getters treat them as not-a-box.
        assert_eq!(js_i32_box_get(state), 0);
        assert_eq!(js_bool_box_get(done), 0);
    }

    /// The intrusive free list must round-trip a WHOLE cohort, not just one
    /// cell. Each free cell's own 8 bytes hold the link to the next, so a
    /// mis-written link would either lose most of the pool (silently
    /// reverting to `std::alloc` and re-growing the residue) or splice a cell
    /// in twice and hand one address to two live activations.
    ///
    /// Asserts all three: every cell comes back, each exactly once, and each
    /// carries its own fresh value rather than a leftover link.
    #[test]
    fn the_intrusive_free_list_round_trips_a_whole_cohort() {
        let _guard = counter_guard();
        super::test_clear_box_registry();
        const N: usize = 512;
        let first: Vec<*mut Box> = (0..N)
            .map(|i| js_box_alloc_bits((i as f64).to_bits() as i64))
            .collect();
        let minted: std::collections::HashSet<usize> = first.iter().map(|p| *p as usize).collect();
        assert_eq!(minted.len(), N, "the fixture must mint N distinct cells");

        for p in &first {
            js_box_release(*p);
        }
        flush_released_boxes();

        let (a0, r0, _) = box_release_stats();
        let second: Vec<*mut Box> = (0..N)
            .map(|i| js_box_alloc_bits((1000.0 + i as f64).to_bits() as i64))
            .collect();
        let (a1, r1, _) = box_release_stats();
        assert_eq!(a1 - a0, N as u64, "second cohort allocates N cells");
        assert_eq!(
            r1 - r0,
            N as u64,
            "ALL N must come from the free list; {} fell through to std::alloc",
            N as u64 - (r1 - r0)
        );

        let reused: std::collections::HashSet<usize> = second.iter().map(|p| *p as usize).collect();
        assert_eq!(reused.len(), N, "an address was handed out twice");
        assert_eq!(
            reused, minted,
            "reused cells must be exactly the minted set"
        );

        for (i, p) in second.iter().enumerate() {
            assert_eq!(
                js_box_get_bits(*p),
                (1000.0 + i as f64).to_bits() as i64,
                "cell {i} kept a stale free-list link instead of its value"
            );
        }
        // Drained: the next allocation has to mint.
        let before = box_release_stats().1;
        let _fresh = js_box_alloc_bits(0);
        assert_eq!(
            box_release_stats().1,
            before,
            "the list was drained, so this must be a fresh std::alloc"
        );
    }

    /// perry#4898 discipline extends to release: a structurally-plausible
    /// pointer that was never minted as a box must be a TOTAL no-op — no
    /// deref, no park.
    #[test]
    fn foreign_pointer_release_is_a_total_noop() {
        super::test_clear_box_registry();
        static RODATA: [u64; 2] = [0xDEAD_BEEF, 0xFEED_FACE];
        let fake = (&RODATA[0] as *const u64) as *mut Box;
        js_box_release(fake);
        assert_eq!(RODATA[0], 0xDEAD_BEEF, "rodata must be untouched");
        let parked = BOX_RELEASE_QUARANTINE.with(|q| q.borrow().len());
        assert_eq!(parked, 0, "foreign pointer must not be parked");
    }

    /// THE #7933-follow-up regression gate, as a counter assertion (the leak
    /// is behaviorally invisible — a test that merely runs to completion
    /// cannot fail on it). Simulate N async-activation lifecycles (alloc a
    /// frame of cells, release it at terminal, hit the drain boundary every
    /// "turn"): the malloc-side residue — cells that cost a real
    /// `std::alloc` allocation, `allocs - pool_reuses` — must stay bounded
    /// by one turn's working set instead of growing linearly with N. Before
    /// the release/reuse machinery existed, residue == every cell ever
    /// allocated (~500 B/activation of cells + registry, 119 MB on
    /// asyncpipe_big).
    #[test]
    fn completed_activation_residue_is_bounded_not_linear() {
        let _guard = counter_guard();
        super::test_clear_box_registry();
        const TURNS: usize = 100;
        const ACTIVATIONS_PER_TURN: usize = 20;
        // handle()-shaped frame: 3 JSValue cells + 1 i32 + 2 bool controls.
        const CELLS_PER_ACTIVATION: usize = 6;
        let (a0, r0, _) = box_release_stats();
        let mut distinct = std::collections::HashSet::new();
        for _ in 0..TURNS {
            for _ in 0..ACTIVATIONS_PER_TURN {
                let b1 = js_box_alloc_bits(crate::value::TAG_UNDEFINED as i64);
                let b2 = js_box_alloc_bits(crate::value::TAG_UNDEFINED as i64);
                let b3 = js_box_alloc_bits(crate::value::TAG_UNDEFINED as i64);
                let state = js_i32_box_alloc(0);
                let done = js_bool_box_alloc(0);
                let exec = js_bool_box_alloc(0);
                for b in [b1, b2, b3] {
                    distinct.insert(b as usize);
                }
                distinct.insert(state as usize);
                distinct.insert(done as usize);
                distinct.insert(exec as usize);
                // Terminal state: release the whole frame.
                for b in [b1, b2, b3] {
                    js_box_release(b);
                }
                js_i32_box_release(state);
                js_bool_box_release(done);
                js_bool_box_release(exec);
            }
            // Outermost microtask-pump boundary, task queue empty.
            flush_released_boxes();
        }
        let (a1, r1, _) = box_release_stats();
        // The counters are process-global; sibling tests on other threads
        // also allocate boxes, so assert lower bounds and give the residue
        // bound slack instead of demanding exact equality.
        let total_allocs = (a1 - a0) as usize;
        let residue = total_allocs - (r1 - r0) as usize;
        let own_allocs = TURNS * ACTIVATIONS_PER_TURN * CELLS_PER_ACTIVATION;
        assert!(
            total_allocs >= own_allocs,
            "every lifecycle allocates its frame ({total_allocs} < {own_allocs})"
        );
        // One turn's working set (the first turn mints real cells; every
        // later turn reuses them), plus generous slack for whatever the
        // parallel sibling tests allocate (they use a handful of cells
        // each). The pre-fix residue is TURNS * the per-turn bound, two
        // orders of magnitude past this.
        let bound = 4 * ACTIVATIONS_PER_TURN * CELLS_PER_ACTIVATION;
        assert!(
            residue <= bound,
            "malloc residue must be bounded by one turn's working set: \
             residue={residue} bound={bound} (linear would be {total_allocs})"
        );
        assert!(
            distinct.len() <= bound,
            "distinct cell addresses must be bounded (got {})",
            distinct.len()
        );
        // The registries hold only the (small) final turn's live set — the
        // linear-growth signature is gone from the scan population too.
        let reg_total = BOX_REGISTRY.with(|r| r.borrow().len())
            + I32_BOX_REGISTRY.with(|r| r.borrow().len())
            + BOOL_BOX_REGISTRY.with(|r| r.borrow().len());
        assert!(
            reg_total <= bound,
            "registry population must not scale with completed activations \
             (got {reg_total})"
        );
    }
}
