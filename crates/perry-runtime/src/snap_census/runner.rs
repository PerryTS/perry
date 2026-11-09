//! The SNAP census runner (`PERRY_SNAP_CENSUS=<out>`): instrumented runtime
//! only. See `snap_census.rs` for the hooks and the design doc §2.3 for the
//! rules this measures.
//!
//! **Run.** On the first module evaluation the runner turns on: it suppresses
//! collection (nothing moves until the boundary, so an address names one
//! object), closes the inline bump bound (every birth goes through
//! `ArenaBlock::alloc`, which logs it), logs the objects that already exist as
//! runtime births, and opens a perf counter of user instructions. Frames are
//! module evaluations (`run_path_initializer`) and promise jobs (a job runs as
//! the init that enqueued it); every access is the top frame's. The run ends
//! at the first explicit `gc()` (the boundary marker), where the report is
//! written, the protections are lifted and the collection runs (with
//! `PERRY_GC_CENSUS` set, its census of the live heap is attributed to births
//! here).
//!
//! **Deferred inits (D).** Rule 1: an init that runs a `host_read` or
//! `host_write` entry, or throws. `PERRY_SNAP_D=<file>` (one evaluation
//! address per line, hex) seeds D with the inits an earlier pass found, so
//! their accesses are recorded from their start: while a member of D is the
//! top frame, every tracked page is `PROT_NONE` and each access is recorded as
//! a read or write of its 8-byte word (`R_D`, `W_D`); while any other init
//! runs, only the pages that hold a `W_D` word (`PROT_NONE`) or an `R_D` word
//! (`PROT_READ`) are protected. A read of a `W_D` word (rule 2) or a write of a
//! `W_D`/`R_D` word (rule 3) makes that init a member from then on; the next
//! pass records it from its start. Passes repeat until D stops growing.
//! Rule 4 (relaxed, `PERRY_SNAP_RULE4=1`): when the first dynamically
//! imported evaluation starts, every word that exists then is watched for
//! writes; an init of the dynamic subgraph that writes one joins D.
//!
//! Faults single-step: the handler opens the page, sets the trap flag, and
//! the `SIGTRAP` after the one instruction closes it again, so the program
//! runs unchanged. Tracked: the GC reservation's mapped regions, the realm
//! image section and the pages of registered module-global root slots.
//!
//! Linux x86_64 only (the reservation, `REG_ERR`, the trap flag).

#[path = "input_record.rs"]
pub(super) mod input_record;
use super::Effect;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, AtomicUsize, Ordering::Relaxed};

const PAGE: usize = 4096;
const MAX_INITS: usize = 1 << 16;

/// The runner's own words, alone on their page: every tracked page that the
/// handler might protect is a page of JS state, never this one.
#[repr(C, align(4096))]
struct Anchor {
    on: AtomicBool,
    started: AtomicU8,
    state: AtomicPtr<State>,
    /// The program's TLS slot ranges (`js_snap_tls_program`): data begin,
    /// data end, bss begin, bss end.
    tls: [AtomicUsize; 4],
    _pad: [u8; PAGE - 48],
}

static ANCHOR: Anchor = Anchor {
    on: AtomicBool::new(false),
    started: AtomicU8::new(0),
    state: AtomicPtr::new(std::ptr::null_mut()),
    tls: [
        AtomicUsize::new(0),
        AtomicUsize::new(0),
        AtomicUsize::new(0),
        AtomicUsize::new(0),
    ],
    _pad: [0; PAGE - 48],
};

/// Census compiles call this first in `main` with the addresses of the four
/// page-aligned anchors the driver links around the program objects
/// (`__perry_snap_tls_{data,bss}_{begin,end}`): the program's module-global
/// TLS slots lie in `[data_begin, data_end)` and `[bss_begin, bss_end)`, on
/// pages of their own. Runtime and libc thread-locals are outside both.
pub(super) fn tls_program(data_lo: usize, data_hi: usize, bss_lo: usize, bss_hi: usize) {
    for (i, v) in [data_lo, data_hi, bss_lo, bss_hi].into_iter().enumerate() {
        ANCHOR.tls[i].store(v, Relaxed);
    }
}

#[inline(always)]
pub(super) fn on() -> bool {
    ANCHOR.on.load(Relaxed)
}

// Const-initialized native TLS needs neither lazy initialization nor allocation.
// It lives outside the generated program's tracked TLS pages.
#[thread_local]
static LOGGING_DEPTH: std::cell::Cell<u32> = std::cell::Cell::new(0);

/// Allocator callbacks must be rejected before formatting or borrowing State.
pub(super) fn inputs_on() -> bool {
    on() && LOGGING_DEPTH.get() == 0
}
pub(super) fn logging_scope(enter: bool) {
    let depth = LOGGING_DEPTH.get();
    LOGGING_DEPTH.set(if enter { depth + 1 } else { depth - 1 });
}

/// Collection is held until the boundary (not in the weights run).
pub(super) fn holds_collection() -> bool {
    on() && !state().noprotect
}

// Native helper threads are visible in the syscall companion, but must not
// borrow the main thread's census buffers.
pub(super) fn on_main_thread() -> bool {
    let p = ANCHOR.state.load(Relaxed);
    !p.is_null() && unsafe { (*p).main_tid == libc::syscall(libc::SYS_gettid) }
}

unsafe fn foreign_input(operation: &[u8], argument: &[u8], value: &[u8]) {
    let header = b"SNAP21_FOREIGN_INPUT";
    let mut rows = [
        libc::iovec {
            iov_base: header.as_ptr() as *mut _,
            iov_len: header.len(),
        },
        libc::iovec {
            iov_base: operation.as_ptr() as *mut _,
            iov_len: operation.len(),
        },
        libc::iovec {
            iov_base: argument.as_ptr() as *mut _,
            iov_len: argument.len(),
        },
        libc::iovec {
            iov_base: value.as_ptr() as *mut _,
            iov_len: value.len(),
        },
    ];
    let saved_errno = *libc::__errno_location();
    libc::syscall(
        libc::SYS_writev,
        -1 as libc::c_int,
        rows.as_mut_ptr(),
        rows.len(),
    );
    *libc::__errno_location() = saved_errno;
}

fn state() -> &'static mut State {
    // SAFETY: set once at start, never freed; the runner is single-threaded
    // (module evaluation and jobs run on the main thread; a fault on another
    // thread only reads it).
    unsafe { &mut *ANCHOR.state.load(Relaxed) }
}

// ---------------------------------------------------------------------------
// Fixed-capacity tables in their own anonymous mappings (never protected, no
// allocation from the signal handler).
// ---------------------------------------------------------------------------

unsafe fn map_zeroed<T>(count: usize) -> *mut T {
    let bytes = count * std::mem::size_of::<T>();
    let p = libc::mmap(
        std::ptr::null_mut(),
        bytes,
        libc::PROT_READ | libc::PROT_WRITE,
        libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE,
        -1,
        0,
    );
    assert!(
        p != libc::MAP_FAILED,
        "snap census: mmap {bytes} bytes failed"
    );
    p.cast()
}

/// A removed [`WordSet`] slot (no word is all ones).
const TOMBSTONE: u64 = u64::MAX;

/// Location ids of the realm global's keys (§2.10.13). Real words are user
/// addresses below 2^47; these ids have the top bit set. `GKEY_ANY`: the
/// global's key set (every add writes it, an unkeyed read reads it: key
/// order and enumeration); `GKEY_UNKEYED`: an unkeyed write (it may change
/// any key: every keyed read reads it); `gkey_loc`: key K.
const GKEY_ANY: u64 = 0x8000_0000_0000_0001;
const GKEY_UNKEYED: u64 = 0x8000_0000_0000_0002;
pub(super) const GKEY_NAME: usize = 40;
const GKEY_CONFLICTS_MAX: usize = 1 << 16;

fn gkey_loc(key: &[u8]) -> u64 {
    // FNV-1a over the key's bytes; the top two bits tag a key id.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in key {
        h = (h ^ b as u64).wrapping_mul(0x0100_0000_01b3);
    }
    let id = (h >> 2) | 0xC000_0000_0000_0000;
    if id == TOMBSTONE {
        id - 1
    } else {
        id
    }
}

/// Open-addressing set of nonzero words.
struct WordSet {
    slots: *mut u64,
    mask: usize,
    len: usize,
}

impl WordSet {
    unsafe fn new(log2: u32) -> Self {
        let n = 1usize << log2;
        WordSet {
            slots: map_zeroed::<u64>(n),
            mask: n - 1,
            len: 0,
        }
    }
    #[inline]
    fn hash(w: u64) -> usize {
        (w.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 17) as usize
    }
    /// True when newly inserted.
    fn insert(&mut self, w: u64) -> bool {
        assert!(self.len * 2 < self.mask, "snap census: word set full");
        let mut i = Self::hash(w) & self.mask;
        unsafe {
            loop {
                let s = *self.slots.add(i);
                if s == w {
                    return false;
                }
                if s == 0 {
                    *self.slots.add(i) = w;
                    self.len += 1;
                    return true;
                }
                i = (i + 1) & self.mask;
            }
        }
    }
    /// Drop `w` (a tombstone keeps later probes intact).
    fn remove(&mut self, w: u64) {
        let mut i = Self::hash(w) & self.mask;
        unsafe {
            loop {
                let s = *self.slots.add(i);
                if s == w {
                    *self.slots.add(i) = TOMBSTONE;
                    return;
                }
                if s == 0 {
                    return;
                }
                i = (i + 1) & self.mask;
            }
        }
    }
    fn contains(&self, w: u64) -> bool {
        let mut i = Self::hash(w) & self.mask;
        unsafe {
            loop {
                let s = *self.slots.add(i);
                if s == w {
                    return true;
                }
                if s == 0 {
                    return false;
                }
                i = (i + 1) & self.mask;
            }
        }
    }
}

/// Open-addressing map from a birth's start to its size.
struct SizeMap {
    keys: *mut u64,
    vals: *mut u32,
    mask: usize,
    len: usize,
}

impl SizeMap {
    unsafe fn new(log2: u32) -> Self {
        let n = 1usize << log2;
        SizeMap {
            keys: map_zeroed::<u64>(n),
            vals: map_zeroed::<u32>(n),
            mask: n - 1,
            len: 0,
        }
    }
    fn insert(&mut self, k: u64, v: u32) {
        assert!(self.len * 2 < self.mask, "snap census: size map full");
        let mut i = WordSet::hash(k) & self.mask;
        unsafe {
            loop {
                let s = *self.keys.add(i);
                if s == k || s == 0 {
                    if s == 0 {
                        self.len += 1;
                    }
                    *self.keys.add(i) = k;
                    *self.vals.add(i) = v;
                    return;
                }
                i = (i + 1) & self.mask;
            }
        }
    }
    fn get(&self, k: u64) -> Option<u32> {
        let mut i = WordSet::hash(k) & self.mask;
        unsafe {
            loop {
                let s = *self.keys.add(i);
                if s == k {
                    return Some(*self.vals.add(i));
                }
                if s == 0 {
                    return None;
                }
                i = (i + 1) & self.mask;
            }
        }
    }
}

/// Open-addressing map from a word to the member of D that recorded it
/// (`init << 32 | pc`, the pc truncated to 32 bits; pc 0 = a binding the
/// member initialized). The cascade attribution: which member's access a
/// later init conflicted with.
struct CauseMap {
    keys: *mut u64,
    vals: *mut u64,
    mask: usize,
    len: usize,
}

impl CauseMap {
    unsafe fn new(log2: u32) -> Self {
        let n = 1usize << log2;
        CauseMap {
            keys: map_zeroed::<u64>(n),
            vals: map_zeroed::<u64>(n),
            mask: n - 1,
            len: 0,
        }
    }
    fn insert(&mut self, k: u64, init: u32, pc: usize) {
        assert!(self.len * 2 < self.mask, "snap census: cause map full");
        let v = (init as u64) << 32 | (pc as u64 & 0xffff_ffff);
        let mut i = WordSet::hash(k) & self.mask;
        unsafe {
            loop {
                let s = *self.keys.add(i);
                if s == k || s == 0 {
                    if s == 0 {
                        self.len += 1;
                    }
                    *self.keys.add(i) = k;
                    *self.vals.add(i) = v;
                    return;
                }
                i = (i + 1) & self.mask;
            }
        }
    }
    fn get(&self, k: u64) -> Option<(u32, usize)> {
        let mut i = WordSet::hash(k) & self.mask;
        unsafe {
            loop {
                let s = *self.keys.add(i);
                if s == k {
                    let v = *self.vals.add(i);
                    return Some(((v >> 32) as u32, (v & 0xffff_ffff) as usize));
                }
                if s == 0 {
                    return None;
                }
                i = (i + 1) & self.mask;
            }
        }
    }
}

/// Append-only log of fixed records.
struct Log<T: Copy> {
    p: *mut T,
    cap: usize,
    len: usize,
}

impl<T: Copy> Log<T> {
    unsafe fn new(cap: usize) -> Self {
        Log {
            p: map_zeroed::<T>(cap),
            cap,
            len: 0,
        }
    }
    fn push(&mut self, v: T) {
        assert!(self.len < self.cap, "snap census: log full");
        unsafe { *self.p.add(self.len) = v };
        self.len += 1;
    }
    fn as_slice(&self) -> &[T] {
        unsafe { std::slice::from_raw_parts(self.p, self.len) }
    }
    fn as_mut_slice(&mut self) -> &mut [T] {
        unsafe { std::slice::from_raw_parts_mut(self.p, self.len) }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// How an evaluation was first reached.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum Reach {
    /// The entry's eager import list (before its body).
    EntryEager = 0,
    /// A static dependency of another evaluation (before that one's body).
    StaticDep = 1,
    /// From a body (an evaluation's, the entry's, or a job's) by `import()`.
    DynImport = 2,
    /// From a body by `require`.
    Require = 3,
}

#[derive(Clone, Copy)]
struct Hit {
    class: u8,
    what: &'static str,
    pcs: [usize; 6],
}

#[derive(Clone, Copy)]
struct Conflict {
    /// 2: read a word a member of D wrote; 3: wrote a word a member of D
    /// read or wrote; 4: a dynamic-subgraph init wrote a word that existed
    /// when the dynamic import started.
    rule: u8,
    word: usize,
    pc: usize,
    write: bool,
    /// The member of D whose recorded access this one conflicts with (its
    /// last write of the word, or its first read), and that access's pc.
    cause: u32,
    cause_pc: usize,
}

#[derive(Clone, Copy)]
struct Init {
    addr: usize,
    /// The emitted once guard of this evaluation; path-wrapper frames have none.
    guard: usize,
    parent: u32,
    reach: Reach,
    /// In the dynamic subgraph: reached by `import()`, or below such an init.
    dynamic: bool,
    /// Reached from the entry's body (as opposed to its eager list).
    from_entry_body: bool,
    enter_seq: u32,
    exit_seq: u32,
    open: bool,
    parts: u8,
    threw: bool,
    seeded: bool,
    transparent: bool,
    ir: u64,
    births: u64,
    birth_bytes: u64,
    host_count: u32,
    host: Option<Hit>,
    conflict: Option<Conflict>,
    in_d: bool,
    faults: u64,
    /// `new Function` bodies and eval scripts made while this init ran.
    dyn_fns: u32,
    evals: u32,
    /// The check run skipped this member's body parts.
    skipped: bool,
    /// The entry module's own evaluation (a program with a worker evaluates
    /// its entry as a module): the entry body. The check run runs it.
    entry_module: bool,
    /// SNAPK2a P-entry: this init's first (a)/(b) hit is logged.
    p_entry_hit: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum FrameKind {
    Init = 0,
    Job = 1,
}

#[derive(Clone, Copy)]
struct Frame {
    init: u32,
    kind: FrameKind,
}

#[derive(Clone, Copy)]
struct Birth {
    addr: usize,
    size: u32,
    init: u32,
    /// The ordered input record's seq when the birth happened (SNAPK2a scans
    /// date a held pointer no earlier than its holder's and target's births).
    seq: u64,
}

/// Page flags (one byte per page).
const PF_W: u8 = 1;
const PF_R: u8 = 2;
const PF_PRE: u8 = 4;
const PF_LISTED: u8 = 8;
const PF_TRACKED: u8 = 16;
/// The page holds watched env words (`env_words`): closed whatever runs.
const PF_ENV: u8 = 32;

/// The evaluation address of the entry body's pseudo-init (design §2.10.7).
const ENTRY_BODY_ADDR: usize = 1;

struct PageMap {
    lo: usize,
    pages: usize,
    flags: *mut u8,
}

impl PageMap {
    unsafe fn new(lo: usize, hi: usize) -> Self {
        let lo = lo & !(PAGE - 1);
        let pages = (hi - lo).div_ceil(PAGE);
        PageMap {
            lo,
            pages,
            flags: map_zeroed::<u8>(pages.max(1)),
        }
    }
    #[inline]
    fn slot(&self, addr: usize) -> Option<*mut u8> {
        let i = addr.wrapping_sub(self.lo) / PAGE;
        (addr >= self.lo && i < self.pages).then(|| unsafe { self.flags.add(i) })
    }
}

struct State {
    inputs: input_record::Record,
    out: String,
    inits: Vec<Init>,
    frames: Vec<Frame>,
    seq: u32,
    seeded: Vec<usize>,
    rule4: bool,
    sabotage_env: bool,
    /// `PERRY_SNAP_DYN=defer`: dynamic code defers the init (design §2.6 (a)).
    dyn_defer: bool,
    /// `PERRY_SNAP_TRANSPARENT`: evaluation bodies whose objects and
    /// globals are not locations (a what-if: the CJS loader's shared state
    /// split per module). Their objects' overflow and meta cells follow.
    transparent: Vec<usize>,
    transp: WordSet,
    transp_no: WordSet,
    transp_globals: WordSet,
    transp_objs: Vec<(usize, usize)>,
    transparent_hits: u64,
    entry_body_seq: u32,
    require_depth: i32,
    /// A literal CommonJS require edge is calling its target's init guard
    /// from this frame's init (`js_snap_require_edge`): the next evaluation
    /// that frame starts was reached by `require`, not by `import()`.
    require_edge: Option<u32>,
    bookkeeping: i32,
    w: WordSet,
    r: WordSet,
    /// Who recorded each `w` / `r` word (the cascade attribution).
    wby: CauseMap,
    rby: CauseMap,
    /// Registered module-global root slots (the handler cannot borrow
    /// `GLOBAL_ROOTS`: the interrupted code may hold it).
    globals: WordSet,
    /// One bit per 8-byte word of the reservation: a birth starts there.
    starts: *mut u64,
    /// Host touches while the runtime (init 0) works for an init: the realm
    /// populating itself on first use. (what, call chain), first per what.
    realm_hosts: Vec<(&'static str, [usize; 6])>,
    excluded: u64,
    /// R1 (design §2.10.16; `PERRY_SNAP_R1=0` turns it off): representation
    /// decides ordering, not ownership. Words a member of D wrote inside a
    /// representation scope, as a meta install or into an excluded cell, of
    /// an object no member of D bore: rolled back, left out of the digest,
    /// no rule-2/3 edge.
    r1: bool,
    repw: WordSet,
    repw_writes: u64,
    /// (word, value before the first rolled-back write).
    repw_old: Log<(u64, u64)>,
    /// The value before the store being classified after its step.
    store_before: Option<u64>,
    repw_owned: u64,
    repw_reads: u64,
    /// Depth of the representation scopes inside `bookkeeping`.
    repr_depth: i32,
    /// The init whose frame was on top at each birth (by start).
    born_by: SizeMap,
    /// R2: a shared immutable cell (string, regex program, canonical key
    /// list) is named by its content and takes no rank. `PERRY_SNAP_R2=0`.
    r2: bool,
    /// Key-name rule (design §2.10.18): a key word is named by what it is
    /// (a leaked symbol by its description), never by its address.
    /// `PERRY_SNAP_KEYNAME=0`.
    keyname: bool,
    /// R3: a WeakStorage digests its entry set, not its buckets. `PERRY_SNAP_R3=0`.
    r3: bool,
    /// Serial rule (design §2.10.17): a dynamic prototype serial is drawn in
    /// allocation order (`proto_validity::next_prototype_serial`), so it is
    /// representation: the digest names it by the birth name of the
    /// prototype that carries it. `PERRY_SNAP_SERIAL=0`.
    serial: bool,
    /// Symbol identity (design §2.10.18): a fresh symbol's id is drawn in
    /// creation order (`symbol::next_id`), so the digest names the id word
    /// by the symbol's own birth name. `PERRY_SNAP_SYMID=0`.
    symid: bool,
    /// First-use rule (design §2.10.20): a cache of static data filled on
    /// first use (a literal class's keys, a regex literal site's matcher
    /// data) is filled by whichever init asks first, D in the census and an
    /// imaged init in the check: the fill is the runtime's (init 0).
    /// `PERRY_SNAP_FIRSTUSE=0`.
    first_use: bool,
    /// Realm rule (design §2.10.20): a realm intrinsic (the global object,
    /// %Object%, the generator and iterator towers, %TypedArray%) is born
    /// lazily inside whichever init first asks for it, D in the census and
    /// an imaged init in the check: its birth and build are the runtime's
    /// (init 0). `PERRY_SNAP_REALM=0`.
    realm_rule: bool,
    /// Slack rule (design §2.10.20): an object's inline width is learned
    /// allocation state (`shapes_birth_width`, `LEARNED_INLINE_FIELDS`) that
    /// D's births and spills move: capacity past the key count is
    /// representation. The digest names an object's shape with its live
    /// bound clipped to its key count and leaves the slack slots out.
    /// `PERRY_SNAP_SLACK=0`.
    slack: bool,
    /// Tail shape rule: an arguments object's private tail
    /// (`ArgumentsTail`: length, birth ShapeId) carries a second copy of a
    /// ShapeId, a creation-order number; it is named by the shape it names,
    /// as the header word is. `PERRY_SNAP_TAILSHAPE=0`.
    tail_shape: bool,
    /// Slot owner rule: a module-global root slot is owned by its first
    /// registrant in the census (the defining module); in the check run a
    /// slot of D's arrives poisoned, and an importer's registration of it is
    /// the duplicate registration, never ownership. `PERRY_SNAP_SLOTOWNER=0`.
    slot_owner_rule: bool,
    /// Backing rule: canonical key lists share one backing per growth chain,
    /// and whether a node sits on a backing with room (and a later list
    /// extends it in place) depends on who built the chain first. A word
    /// into a canonical backing outside a shape word names the chain by its
    /// first key; the node's count is checked where it lives (shape words).
    /// `PERRY_SNAP_BACKING=0`.
    backing: bool,
    /// Check order rule: an imaged init the census entered inside the body
    /// of a member of D runs in the check where that body is skipped, in
    /// entry order (`<census>.dkids`, beside the D file the check reads).
    /// `PERRY_SNAP_DKIDS=0`.
    dkids: Vec<(usize, usize, usize)>,
    /// D-written slot rule: a module-global slot a member of D wrote is
    /// left out of the census digest and named in `.wd` (`0<TAB>slot`, the
    /// dig's own name for a slot), so the check leaves it out too, as a
    /// birth word D wrote. `PERRY_SNAP_WDSLOT=0`.
    wd_slot: bool,
    /// Size of every birth and realm image cell, by start.
    sizes: SizeMap,
    /// One bit per 8-byte word of the realm section: an image cell starts.
    realm_starts: *mut u64,
    free_space: u64,
    reborn: u64,
    silent_stores: u64,
    noprotect: bool,
    /// The meta rule (design §2.10.11; `PERRY_SNAP_META=0` turns it off):
    /// installing an empty meta record and marking an object a prototype
    /// are representation, and a meta record's words are its owner's.
    meta_rule: bool,
    /// The owner's meta word whose (empty) record is being stored now.
    meta_slot: usize,
    /// Meta installs seen; records of pre-existing owners added to rule 4's
    /// pre-existing set; classification marks (representation scopes).
    meta_installs: u64,
    /// Key-granular globalThis (design §2.10.13; `PERRY_SNAP_GKEY=0` turns
    /// it off): the realm global's shape word is no location by itself; a
    /// keyed operation on the global (a lookup of key X, the add or store of
    /// key K) names the key it accesses, and an unkeyed access stands for
    /// every key. `gshape`: the global's shape word (0 until it exists).
    gkey_rule: bool,
    gshape: usize,
    /// The key the running keyed operation on the global names (a location
    /// id, 0: none) and its name for the report.
    gkey: u64,
    gkey_name: [u8; GKEY_NAME],
    gkey_len: u8,
    /// Shape-word accesses: keyed reads, keyed writes, unkeyed reads,
    /// unkeyed writes.
    gkey_counts: [u64; 4],
    /// The key names of keyed conflicts, per init (filled at the conflict).
    gkey_conflicts: Vec<(u32, u64, [u8; GKEY_NAME], u8)>,
    meta_owned: u64,
    marks: u64,
    /// `js_snap_entry_module`: the next evaluation is the entry module's own
    /// (its body is the entry body, design §2.3).
    entry_module_next: bool,
    /// Global root slots on no tracked page that are not the runtime's own
    /// thread-locals (a program slot the census does not watch): must be 0.
    untracked_slots: u64,
    /// The first of them (0: none), for the report.
    untracked_first: usize,
    /// Roots the runtime registers in its own thread-locals (inside the
    /// static TLS block, outside the program's anchored pages, e.g. the realm
    /// global's cache word): representation, counted on their own report
    /// line, never protected.
    runtime_tls_roots: u64,
    runtime_tls_first: usize,
    /// The executable's static TLS block `[fs - PT_TLS size, fs)`.
    tls_block: (usize, usize),
    /// The program's TLS slots are anchored on whole pages of their own.
    tls_whole: bool,
    /// `PERRY_SNAP_MODE=top`: every member of D may read and write every
    /// location that exists when it starts (a ⊤ summary). Members run
    /// untracked; any other init that touches such a location joins D.
    top_mode: bool,
    /// One bit per reservation word, at birth starts: born before the latest
    /// member's start (the ⊤ pre-existing set).
    pre_bits: *mut u64,
    pre_upto: usize,
    globals_log: Vec<usize>,
    pre_globals: WordSet,
    pre_globals_upto: usize,
    top_armed: bool,
    births: Log<Birth>,
    listed: Log<usize>,
    regions: Vec<(usize, usize)>,
    heap: PageMap,
    bin: PageMap,
    /// The main thread's program TLS slots (initialized data): module
    /// globals live there in a program with a Worker (per-thread init flags
    /// and bindings, #10399). Only the anchored range (`tls_program`).
    tls: PageMap,
    /// The same for the program's zero-initialized TLS slots.
    tlsb: PageMap,
    tls_ranges: [usize; 4],
    /// Pages opened for the current instruction (the handler's own state is
    /// never in TLS: a TLS page may be protected).
    pending: [usize; 8],
    /// Stores the faulting instruction is about to make: (word, value
    /// before, pc). Classified once it has run (see `on_trap`).
    stores: [(usize, u64, usize); 4],
    rset_word: usize,
    /// Handler depth: a fault inside the handler is reported, not hidden.
    in_handler: u32,
    /// The thread that runs module evaluation; the runner tracks only its
    /// memory (a worker agent has its own heap regions).
    main_tid: i64,
    /// Single steps of other threads: (tid, page). Those accesses belong
    /// to no init; they are stepped through and counted.
    foreign: [(i64, usize); 64],
    foreign_faults: u64,
    realm: (usize, usize),
    d_mode: bool,
    perf_fd: i32,
    last_ir: u64,
    /// Birth-log length when the first dynamic import started (rule 4).
    pre_births: usize,
    pre_sorted: Vec<(usize, usize)>,
    rule4_armed: bool,
    host_outside: u64,
    faults_total: u64,
    faults_ignored: u64,
    boundary_seq: u32,
    boundary_at: &'static str,
    // census at the boundary collection
    live_by: std::collections::HashMap<(u32, u8), [u64; 2]>,
    live_unattributed: [u64; 2],
    boundary_done: bool,
    /// Ground truth: every `getenv` by any code while the census runs
    /// (init 0 included), with the value it found.
    env_reads: Vec<(u32, String, Option<String>)>,
    /// SNAP3 (b): env reads are cells or guard members, not rule 1
    /// (`PERRY_SNAP_ENV=0`: rule 1 as before).
    env_rule: bool,
    /// SNAP3 (a): write containment for the `import()` subgraph
    /// (`PERRY_SNAP_Q7=0`: every init of the subgraph defers).
    q7: bool,
    /// The kind `js_snap_env_site` left for the frame's next `js_getenv`.
    env_site: Option<(u32, u8)>,
    env_obj: u64,
    env_obj_last: u32,
    env_obs: Vec<EnvObs>,
    /// Watched words: the env block (`ENV_BLOCK`), store-site string bodies
    /// (their `env_obs` index), poisoned slots (`POISON`).
    env_words: SizeMap,
    env_storage: Vec<(usize, usize)>,
    /// The init that registered each `globals_log` slot.
    slot_owner: Vec<u32>,
    /// The check run (`PERRY_SNAP_SKIP=<D file>`): D's evaluation addresses
    /// (sorted), whose bodies are skipped.
    skip: Vec<usize>,
    /// The check run's poisoned-binding reads. Pushed in signal context:
    /// the capacity is reserved up front (a push that grows would allocate
    /// inside the fault handler).
    check_fail: Vec<(u32, usize, usize, bool)>,
    /// The check run writes each poisoned read to `<out>.checkfail` as it
    /// happens (signal-safe), so a run that never reaches the boundary
    /// still leaves its report.
    checkfail_fd: i32,
    /// The check run's digest leaves out the words the census pass's D wrote
    /// (`PERRY_SNAP_WD`, by logical name).
    wd_names: std::collections::HashSet<(u64, usize)>,
    entry_init: u32,
    /// SNAPK2a (DESIGN27 §1.3, §2). Host values (`PERRY_SNAP_HOST=0` off):
    /// a `process.argv`/`process.execPath` read is live in the entry and a
    /// STOP anywhere else; a word outside entry state that holds a pointer
    /// into a host value is a STOP.
    host_rule: bool,
    /// P-entry (`PERRY_SNAP_PENTRY=0` off): no imaged init reads a word an
    /// entry wrote (a), writes a word an entry read (b), or holds a pointer
    /// to an entry birth (c).
    p_entry_rule: bool,
    /// P-reach (`PERRY_SNAP_PREACH=0` off): an init an entry reaches by
    /// `import()`/`require` names the entry's inputs read before it.
    p_reach_rule: bool,
    /// Words an entry read (P-entry (b)).
    er: WordSet,
    /// Words whose last writer is an entry (P-entry (a)). `w`/`wby` cannot
    /// say it: outside D they record only a conflicting init's accesses.
    ew: WordSet,
    /// First (a)/(b) hit per init: (init, rule, word, pc, input seq).
    /// Pushed in signal context: the capacity is reserved up front.
    p_entry_log: Vec<(u32, u8, usize, usize, u64)>,
    /// The open host-value scope (1 argv, 2 execPath; 0 none) and the births
    /// made inside one: (start, end, what, input seq).
    host_what: u8,
    host_targets: Vec<(usize, usize, u8, u64)>,
    /// The entry's input rows since the last reach point: (seq, op).
    entry_rows: Vec<(u64, String)>,
    mark: bool,
    /// R5 (`PERRY_SNAP_RSET=1`): after the boundary, the imaged cells stay
    /// write-protected and their writes are counted until exit.
    rset: bool,
    rset_active: bool,
    /// Imaged births, sorted by address: (start, end).
    cand: Vec<(usize, usize)>,
    cand_writes: Vec<u32>,
    cand_ptr_writes: Vec<u32>,
    rset_faults: u64,
    t0: std::time::Instant,
}

// ---------------------------------------------------------------------------
// Start
// ---------------------------------------------------------------------------

pub(super) fn start_if_requested() -> bool {
    match ANCHOR.started.load(Relaxed) {
        1 => return false,
        2 => return true,
        _ => {}
    }
    let Ok(out) = std::env::var("PERRY_SNAP_CENSUS") else {
        ANCHOR.started.store(1, Relaxed);
        return false;
    };
    if out.is_empty() {
        ANCHOR.started.store(1, Relaxed);
        return false;
    }
    ANCHOR.started.store(2, Relaxed);
    unsafe { start_census(out) };
    true
}

// Not `start`: scripts/gc_runtime_root_holders.py walks function names, and a
// census fn named like a scanner callee would read GLOBAL_ROOTS as covered.
unsafe fn start_census(out: String) {
    let seeded: Vec<usize> = std::env::var("PERRY_SNAP_D")
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| {
            t.lines()
                .filter_map(|l| usize::from_str_radix(l.trim().trim_start_matches("0x"), 16).ok())
                .collect()
        })
        .unwrap_or_default();
    let (res_lo, res_hi) = crate::arena::snap_reservation_bounds();
    let realm = crate::value::addr_class::realm::bounds();
    let (bin_lo, bin_hi) = writable_image_range(realm.0);
    // The program's TLS slots: only the anchored ranges, whole pages that hold
    // program slots and nothing else (runtime thread-locals, libc's TLS, the
    // TCB and its rseq area are outside them, so the handler path may touch
    // them freely). Without the anchors (not a census compile) no TLS page is
    // tracked and every TLS slot counts as untracked.
    let t: [usize; 4] = std::array::from_fn(|i| ANCHOR.tls[i].load(Relaxed));
    let tls_whole = t[0] != 0 && t.iter().all(|&a| a % PAGE == 0) && t[0] <= t[1] && t[2] <= t[3];
    let (tls_lo, tls_hi, tlsb_lo, tlsb_hi) = if tls_whole {
        (t[0], t[1], t[2], t[3])
    } else {
        (0, 0, 0, 0)
    };
    // x86-64 variant II: the static TLS block ends at the thread pointer.
    let mut fs = 0usize;
    libc::syscall(
        libc::SYS_arch_prctl,
        0x1003, /* ARCH_GET_FS */
        &mut fs as *mut usize,
    );
    let tls_block = (fs.saturating_sub(executable_tls_size()), fs);
    let mut st = Box::new(State {
        inputs: input_record::Record::default(),
        out,
        inits: Vec::with_capacity(4096),
        frames: Vec::with_capacity(256),
        seq: 0,
        seeded,
        rule4: !std::env::var("PERRY_SNAP_Q7").is_ok_and(|v| v == "0"),
        sabotage_env: std::env::var("PERRY_SNAP_SABOTAGE").is_ok_and(|v| v == "env"),
        dyn_defer: std::env::var("PERRY_SNAP_DYN").is_ok_and(|v| v == "defer"),
        transparent: std::env::var("PERRY_SNAP_TRANSPARENT")
            .unwrap_or_default()
            .split(',')
            .filter_map(|a| usize::from_str_radix(a.trim().trim_start_matches("0x"), 16).ok())
            .collect(),
        transp: WordSet::new(18),
        transp_no: WordSet::new(20),
        transp_globals: WordSet::new(14),
        transp_objs: Vec::with_capacity(1 << 15),
        transparent_hits: 0,
        entry_body_seq: 0,
        require_depth: 0,
        require_edge: None,
        bookkeeping: 0,
        w: WordSet::new(28),
        r: WordSet::new(28),
        wby: CauseMap::new(28),
        rby: CauseMap::new(28),
        globals: WordSet::new(22),
        starts: map_zeroed::<u64>(((res_hi - res_lo) / 8).div_ceil(64).max(1)),
        realm_hosts: Vec::new(),
        excluded: 0,
        r1: !std::env::var("PERRY_SNAP_R1").is_ok_and(|v| v == "0"),
        repw: WordSet::new(24),
        repw_writes: 0,
        repw_old: Log::new(1 << 24),
        store_before: None,
        repw_owned: 0,
        repw_reads: 0,
        repr_depth: 0,
        born_by: SizeMap::new(26),
        r2: !std::env::var("PERRY_SNAP_R2").is_ok_and(|v| v == "0"),
        keyname: !std::env::var("PERRY_SNAP_KEYNAME").is_ok_and(|v| v == "0"),
        r3: !std::env::var("PERRY_SNAP_R3").is_ok_and(|v| v == "0"),
        serial: !std::env::var("PERRY_SNAP_SERIAL").is_ok_and(|v| v == "0"),
        symid: !std::env::var("PERRY_SNAP_SYMID").is_ok_and(|v| v == "0"),
        slack: !std::env::var("PERRY_SNAP_SLACK").is_ok_and(|v| v == "0"),
        tail_shape: !std::env::var("PERRY_SNAP_TAILSHAPE").is_ok_and(|v| v == "0"),
        first_use: !std::env::var("PERRY_SNAP_FIRSTUSE").is_ok_and(|v| v == "0"),
        realm_rule: !std::env::var("PERRY_SNAP_REALM").is_ok_and(|v| v == "0"),
        slot_owner_rule: !std::env::var("PERRY_SNAP_SLOTOWNER").is_ok_and(|v| v == "0"),
        backing: !std::env::var("PERRY_SNAP_BACKING").is_ok_and(|v| v == "0"),
        dkids: if std::env::var("PERRY_SNAP_DKIDS").is_ok_and(|v| v == "0") {
            Vec::new()
        } else {
            std::env::var("PERRY_SNAP_SKIP")
                .ok()
                .and_then(|p| std::fs::read_to_string(format!("{}kids", p)).ok())
                .map(|t| {
                    t.lines()
                        .filter_map(|l| {
                            let mut row = l.split('\t');
                            Some((
                                usize::from_str_radix(row.next()?, 16).ok()?,
                                usize::from_str_radix(row.next()?, 16).ok()?,
                                usize::from_str_radix(row.next().unwrap_or("0"), 16).ok()?,
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default()
        },
        wd_slot: !std::env::var("PERRY_SNAP_WDSLOT").is_ok_and(|v| v == "0"),
        sizes: SizeMap::new(26),
        realm_starts: map_zeroed::<u64>(((realm.1 - realm.0) / 8).div_ceil(64).max(1)),
        free_space: 0,
        reborn: 0,
        silent_stores: 0,
        noprotect: std::env::var("PERRY_SNAP_NOPROTECT").is_ok_and(|v| v == "1"),
        meta_rule: !std::env::var("PERRY_SNAP_META").is_ok_and(|v| v == "0"),
        meta_slot: 0,
        meta_installs: 0,
        gkey_rule: !std::env::var("PERRY_SNAP_GKEY").is_ok_and(|v| v == "0"),
        gshape: 0,
        gkey: 0,
        gkey_name: [0; GKEY_NAME],
        gkey_len: 0,
        gkey_counts: [0; 4],
        gkey_conflicts: Vec::with_capacity(GKEY_CONFLICTS_MAX),
        meta_owned: 0,
        marks: 0,
        entry_module_next: false,
        untracked_slots: 0,
        untracked_first: 0,
        runtime_tls_roots: 0,
        runtime_tls_first: 0,
        tls_block,
        tls_whole,
        top_mode: std::env::var("PERRY_SNAP_MODE").is_ok_and(|v| v == "top"),
        pre_bits: map_zeroed::<u64>(((res_hi - res_lo) / 8).div_ceil(64).max(1)),
        pre_upto: 0,
        globals_log: Vec::new(),
        pre_globals: WordSet::new(22),
        pre_globals_upto: 0,
        top_armed: false,
        births: Log::new(1 << 28),
        listed: Log::new(1 << 26),
        regions: Vec::new(),
        heap: PageMap::new(res_lo, res_hi),
        bin: PageMap::new(bin_lo, bin_hi),
        tls: PageMap::new(tls_lo, tls_hi),
        tlsb: PageMap::new(tlsb_lo, tlsb_hi),
        tls_ranges: t,
        pending: [0; 8],
        stores: [(0, 0, 0); 4],
        rset_word: 0,
        in_handler: 0,
        main_tid: libc::syscall(libc::SYS_gettid),
        foreign: [(0, 0); 64],
        foreign_faults: 0,
        realm,
        d_mode: false,
        perf_fd: open_instruction_counter(),
        last_ir: 0,
        pre_births: usize::MAX,
        pre_sorted: Vec::new(),
        rule4_armed: false,
        host_outside: 0,
        faults_total: 0,
        faults_ignored: 0,
        boundary_seq: 0,
        boundary_at: "",
        live_by: std::collections::HashMap::new(),
        live_unattributed: [0; 2],
        boundary_done: false,
        env_reads: Vec::new(),
        env_rule: !std::env::var("PERRY_SNAP_ENV").is_ok_and(|v| v == "0"),
        q7: !std::env::var("PERRY_SNAP_Q7").is_ok_and(|v| v == "0"),
        env_site: None,
        env_obj: 0,
        env_obj_last: 0,
        env_obs: Vec::new(),
        env_words: SizeMap::new(20),
        env_storage: Vec::new(),
        slot_owner: Vec::new(),
        skip: {
            let mut v = std::env::var("PERRY_SNAP_SKIP")
                .map(|p| read_hex_lines(&p))
                .unwrap_or_default();
            v.sort_unstable();
            v
        },
        check_fail: Vec::with_capacity(CHECK_FAIL_MAX),
        checkfail_fd: -1,
        wd_names: std::env::var("PERRY_SNAP_WD")
            .ok()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|t| {
                t.lines()
                    .filter_map(|l| {
                        let (n, o) = l.split_once('\t')?;
                        Some((u64::from_str_radix(n, 16).ok()?, o.parse().ok()?))
                    })
                    .collect()
            })
            .unwrap_or_default(),
        entry_init: 0,
        host_rule: !std::env::var("PERRY_SNAP_HOST").is_ok_and(|v| v == "0"),
        p_entry_rule: !std::env::var("PERRY_SNAP_PENTRY").is_ok_and(|v| v == "0"),
        p_reach_rule: !std::env::var("PERRY_SNAP_PREACH").is_ok_and(|v| v == "0"),
        er: WordSet::new(24),
        ew: WordSet::new(24),
        p_entry_log: Vec::with_capacity(CHECK_FAIL_MAX),
        host_what: 0,
        host_targets: Vec::new(),
        entry_rows: Vec::new(),
        mark: std::env::var("PERRY_SNAP_MARK").is_ok_and(|v| v == "1"),
        rset: std::env::var("PERRY_SNAP_RSET").is_ok_and(|v| v == "1"),
        rset_active: false,
        cand: Vec::new(),
        cand_writes: Vec::new(),
        cand_ptr_writes: Vec::new(),
        rset_faults: 0,
        t0: std::time::Instant::now(),
    });
    // Init 0: the runtime's own start and the entry's body.
    st.inits.push(blank_init(0, 0, Reach::EntryEager));
    st.last_ir = read_ir(st.perf_fd);
    ANCHOR.state.store(&mut *st as *mut State, Relaxed);
    std::mem::forget(st);
    let _input_logging = input_record::Scope::enter();
    let st = state();
    // Nothing moves until the boundary. The weights run (no protection)
    // collects as production does: its Ir per init includes the collections.
    if !st.noprotect {
        crate::gc::gc_suppress();
    }
    // The objects that exist now are the runtime's (init 0).
    crate::arena::arena_walk_objects_with_block_index(|header, _| {
        let size = (*(header as *const crate::gc::GcHeader)).size as usize;
        set_start(st, header as usize);
        st.sizes.insert(header as u64, size as u32);
        st.births.push(Birth {
            addr: header as usize,
            size: size as u32,
            init: 0,
            seq: 0,
        });
    });
    crate::arena::snap_reservation_regions(|base, end| st.regions.push((base, end)));
    for &(b, e) in &st.regions.clone() {
        mark_tracked(b, e);
    }
    mark_tracked(realm.0, realm.1);
    let mut slots = Vec::new();
    crate::gc::for_each_global_root_slot(|slot| slots.push(slot));
    for slot in slots {
        count_unwatched_root(&mut *st, slot);
        st.globals.insert(slot as u64);
        st.globals_log.push(slot);
        st.slot_owner.push(0);
        mark_tracked(slot, slot + 8);
    }
    walk_realm_cells(st);
    // The check run's report file, opened before any page closes: this
    // allocates, and the handlers are not installed yet.
    if !st.skip.is_empty() {
        let path = format!("{}.checkfail\0", st.out);
        st.checkfail_fd = libc::open(
            path.as_ptr().cast(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC | libc::O_CLOEXEC,
            0o644,
        );
        libc::atexit(check_exit);
    }
    // The check run: the global slots the census pass's D registered are
    // poisoned (any access is a check failure).
    if let Ok(p) = std::env::var("PERRY_SNAP_POISON") {
        for slot in read_hex_lines(&p) {
            if page_flags(st, slot).is_some() {
                mark_tracked(slot, slot + 8);
                watch_env_words(st, slot, slot + 8, POISON);
            }
        }
    }
    st.inputs.watch_enabled = !std::env::var("PERRY_SNAP22_WATCH").is_ok_and(|v| v == "0");
    C25.store(
        !std::env::var("PERRY_SNAP25").is_ok_and(|v| v == "0"),
        Relaxed,
    );
    st.inputs.allocated_bytes = st.births.as_slice().iter().map(|b| b.size as u64).sum();
    input_record::open(st).expect("cannot open ordered input record");
    install_handlers();
    ANCHOR.on.store(true, Relaxed);
    let _input_logging = input_record::Scope::enter();
    input_record::event(st, "census_start", String::new());
    let mut raw_env = Vec::<Vec<u8>>::new();
    let mut env_entry = environ;
    while !env_entry.is_null() && !(*env_entry).is_null() {
        raw_env.push(std::ffi::CStr::from_ptr(*env_entry).to_bytes().to_vec());
        env_entry = env_entry.add(1);
    }
    input_record::event(
        st,
        "initial_environment",
        input_record::Value::input_value(&raw_env),
    );
    crate::arena::refresh_inline_allocation_limit();
}

/// Record the realm section's image cells (header + size, back to back,
/// zero padding between them skipped).
unsafe fn walk_realm_cells(st: &mut State) {
    let (lo, hi) = st.realm;
    let mut p = (lo + 7) & !7;
    while p + 8 <= hi {
        let g = &*(p as *const crate::gc::GcHeader);
        let size = g.size as usize;
        if size >= 8 && size % 8 == 0 && p + size <= hi && g.obj_type != 0 {
            let i = (p - lo) / 8;
            *st.realm_starts.add(i / 64) |= 1 << (i % 64);
            st.sizes.insert(p as u64, size as u32);
            p += size;
        } else {
            p += 8;
        }
    }
}

/// The realm image cell that holds `word`.
fn realm_cell(st: &State, word: usize) -> Option<usize> {
    let (lo, hi) = st.realm;
    if word < lo || word >= hi {
        return None;
    }
    let mut i = (word - lo) / 8;
    loop {
        let w = unsafe { *st.realm_starts.add(i / 64) } & (u64::MAX >> (63 - (i % 64)));
        if w != 0 {
            let bit = 63 - w.leading_zeros() as usize;
            return Some(lo + ((i / 64) * 64 + bit) * 8);
        }
        if i < 64 {
            return None;
        }
        i = i - (i % 64) - 1;
    }
}

fn blank_init(addr: usize, parent: u32, reach: Reach) -> Init {
    Init {
        addr,
        guard: 0,
        parent,
        reach,
        dynamic: false,
        from_entry_body: false,
        enter_seq: 0,
        exit_seq: 0,
        open: false,
        parts: 0,
        threw: false,
        seeded: false,
        transparent: false,
        ir: 0,
        births: 0,
        birth_bytes: 0,
        host_count: 0,
        host: None,
        conflict: None,
        dyn_fns: 0,
        evals: 0,
        in_d: false,
        faults: 0,
        skipped: false,
        entry_module: false,
        p_entry_hit: false,
    }
}

/// The executable's writable mapping(s) around `inside` (its .data/.bss,
/// where module globals and the realm section live), from /proc/self/maps.
fn writable_image_range(inside: usize) -> (usize, usize) {
    let maps = std::fs::read_to_string("/proc/self/maps").unwrap_or_default();
    let mut rows: Vec<(usize, usize, bool)> = Vec::new();
    for line in maps.lines() {
        let mut it = line.split_whitespace();
        let (Some(range), Some(perms)) = (it.next(), it.next()) else {
            continue;
        };
        let Some((a, b)) = range.split_once('-') else {
            continue;
        };
        let (Ok(a), Ok(b)) = (usize::from_str_radix(a, 16), usize::from_str_radix(b, 16)) else {
            continue;
        };
        rows.push((a, b, perms.starts_with("rw")));
    }
    let Some(i) = rows.iter().position(|&(a, b, _)| a <= inside && inside < b) else {
        return (inside, inside + PAGE);
    };
    let (mut lo, mut hi) = (rows[i].0, rows[i].1);
    let mut j = i;
    while j > 0 && rows[j - 1].2 && rows[j - 1].1 == lo {
        j -= 1;
        lo = rows[j].0;
    }
    let mut k = i;
    while k + 1 < rows.len() && rows[k + 1].2 && rows[k + 1].0 == hi {
        k += 1;
        hi = rows[k].1;
    }
    (lo, hi)
}

fn open_instruction_counter() -> i32 {
    // perf_event_attr, version 0 layout (64 bytes).
    let mut attr = [0u64; 8];
    let type_and_size: u64 = /* PERF_TYPE_HARDWARE */ 0 | (64u64 << 32);
    attr[0] = type_and_size;
    attr[1] = 1; // PERF_COUNT_HW_INSTRUCTIONS
    attr[5] = (1 << 5) | (1 << 6); // exclude_kernel | exclude_hv
    let fd = unsafe {
        libc::syscall(
            libc::SYS_perf_event_open,
            attr.as_ptr(),
            0 as libc::pid_t,
            -1 as libc::c_int,
            -1 as libc::c_int,
            0 as libc::c_ulong,
        )
    };
    fd as i32
}

fn read_ir(fd: i32) -> u64 {
    if fd < 0 {
        return 0;
    }
    let mut v = 0u64;
    unsafe { libc::read(fd, (&mut v as *mut u64).cast(), 8) };
    v
}

// ---------------------------------------------------------------------------
// Frames
// ---------------------------------------------------------------------------

fn charge(st: &mut State) {
    let now = read_ir(st.perf_fd);
    let top = st.frames.last().map_or(0, |f| f.init);
    st.inits[top as usize].ir += now.saturating_sub(st.last_ir);
    st.last_ir = now;
}

pub(super) fn current_init() -> u32 {
    state().frames.last().map_or(0, |f| f.init)
}

fn set_mode_for_top(st: &mut State) {
    let top = st.frames.last().map_or(0, |f| f.init) as usize;
    if st.mark {
        // A syscall trace (strace) attributes what follows to this init.
        unsafe {
            libc::syscall(
                libc::SYS_lseek,
                -1 as libc::c_int,
                top as libc::c_long,
                0x5a,
            )
        };
    }
    let want = top != 0 && st.inits[top].in_d && !st.noprotect;
    let watching = input_record::watch(st);
    if want != st.d_mode || watching != st.inputs.watching {
        st.inputs.watching = watching;
        st.d_mode = want;
        unsafe { apply_protection(st) };
    }
}

pub(super) fn init_enter(addr: usize) {
    if !on() && !start_if_requested() {
        return;
    }
    let _input_logging = input_record::Scope::enter();
    let st = state();
    charge(st);
    let parent = st.frames.last().copied();
    let parent_id = parent.map_or(0, |f| f.init);
    let parent_in_body = match parent {
        None => st.entry_body_seq != 0,
        Some(f) if f.kind == FrameKind::Job => true,
        Some(f) => {
            st.inits[f.init as usize].parts >= 2
                || st.inits[f.init as usize].addr == 0
                || st.inits[f.init as usize].addr == ENTRY_BODY_ADDR
        }
    };
    let edge = st.require_edge.take() == Some(parent_id);
    let entry_module = std::mem::take(&mut st.entry_module_next)
        && st.entry_init != 0
        && parent_id == st.entry_init;
    let reach = if entry_module {
        Reach::EntryEager
    } else if st.require_depth > 0 || edge {
        Reach::Require
    } else if !parent_in_body {
        if parent.is_none() {
            Reach::EntryEager
        } else {
            Reach::StaticDep
        }
    } else {
        Reach::DynImport
    };
    // P-reach (DESIGN27 §2): the entry reached this init by control flow;
    // what the entry read before it are required guard members. The row
    // follows `init_enter`: the charge's counter read must stay the last one
    // before `init_enter` (the exact Ir accounting joins them).
    let reach_row = (st.p_reach_rule
        && !entry_module
        && matches!(reach, Reach::DynImport | Reach::Require)
        && is_entry(st, parent_id))
    .then(|| {
        let members: Vec<String> = st
            .entry_rows
            .drain(..)
            .map(|(s, op)| format!("{s}:{op}"))
            .collect();
        format!(
            "reach_init={};addr={addr:x};members={}",
            st.inits.len(),
            members.join(",")
        )
    });
    st.seq += 1;
    let id = st.inits.len() as u32;
    assert!((id as usize) < MAX_INITS, "snap census: too many inits");
    let mut init = blank_init(addr, parent_id, reach);
    init.enter_seq = st.seq;
    init.open = true;
    let parent_dynamic = parent_id != 0 && st.inits[parent_id as usize].dynamic;
    init.dynamic = !entry_module && (reach == Reach::DynImport || parent_dynamic);
    if entry_module {
        // The entry module's evaluation is the entry body (§2.10.7): in D,
        // reached at startup in every run, never part of the import() subgraph.
        init.entry_module = true;
        init.in_d = true;
        init.host = Some(Hit {
            class: Effect::HostRead as u8,
            what: "entry_body",
            pcs: [0; 6],
        });
    }
    if init.dynamic && !st.q7 {
        // Without write containment an init of the `import()` subgraph has
        // no proof of its own: it runs at the import, after the entry body.
        init.in_d = true;
        init.host = Some(Hit {
            class: Effect::HostWrite as u8,
            what: "dyn_import",
            pcs: [0; 6],
        });
    }
    init.from_entry_body = (parent.is_none() && st.entry_body_seq != 0)
        || (parent_id != 0 && st.inits[parent_id as usize].from_entry_body);
    init.seeded = st.seeded.contains(&addr);
    init.transparent = st.transparent.contains(&addr);
    init.in_d = init.seeded || init.in_d;
    if st.top_mode && init.in_d {
        unsafe { extend_top(st) };
    }
    if init.dynamic && st.rule4 && !st.rule4_armed {
        unsafe { arm_rule4(st) };
    }
    st.inits.push(init);
    st.frames.push(Frame {
        init: id,
        kind: FrameKind::Init,
    });
    crate::object::shapes::snap22::snapshot("init_enter", addr as u64);
    input_record::event(st, "init_enter", format!("{addr:x}"));
    if let Some(row) = reach_row {
        input_record::event(st, "p_reach", row);
    }
    set_mode_for_top(st);
}

pub(super) fn init_exit(threw: bool) {
    let _input_logging = input_record::Scope::enter();
    let st = state();
    charge(st);
    input_record::event(st, "init_exit", format!("threw={threw}"));
    let Some(f) = st.frames.pop() else {
        return;
    };
    st.seq += 1;
    let i = &mut st.inits[f.init as usize];
    i.open = false;
    i.exit_seq = st.seq;
    if threw {
        i.threw = true;
        i.in_d = true;
    }
    set_mode_for_top(st);
}

/// Attach the evaluation's emitted once guard after its guarded call returns.
/// The existing init record owns this property, including failed evaluations.
pub(super) fn init_guard(addr: usize, guard: usize) {
    if let Some(i) = state().inits.iter_mut().rfind(|i| i.addr == addr) {
        i.guard = guard;
    }
}

pub(super) fn init_part() {
    let _input_logging = input_record::Scope::enter();
    let st = state();
    if let Some(f) = st.frames.last() {
        if f.kind == FrameKind::Init {
            let i = &mut st.inits[f.init as usize];
            i.parts = i.parts.saturating_add(1);
        }
    }
    input_record::event(st, "init_part", String::new());
    set_mode_for_top(st);
}

pub(super) fn require_depth(delta: i32) {
    state().require_depth += delta;
}

/// `js_snap_require_edge`: 1 before a literal CommonJS require edge calls its
/// target's init guard, 0 after it (the target was already evaluated, so no
/// evaluation took the mark).
pub(super) fn require_edge(on: bool) {
    let _input_logging = input_record::Scope::enter();
    let st = state();
    st.require_edge = on.then(|| top_init(st));
}

/// The first-use rule is on (`PERRY_SNAP_FIRSTUSE` is not `0`).
pub(super) fn first_use_on() -> bool {
    state().first_use
}

/// The realm rule is on (`PERRY_SNAP_REALM` is not `0`).
pub(super) fn realm_on() -> bool {
    state().realm_rule
}

pub(super) fn bookkeeping_depth(delta: i32) {
    state().bookkeeping += delta;
}

pub(super) fn job_enter(owner: u32) {
    let _input_logging = input_record::Scope::enter();
    let st = state();
    charge(st);
    st.frames.push(Frame {
        init: owner,
        kind: FrameKind::Job,
    });
    input_record::event(st, "job_enter", owner.to_string());
    set_mode_for_top(st);
}

pub(super) fn job_exit() {
    let _input_logging = input_record::Scope::enter();
    let st = state();
    input_record::event(st, "job_exit", String::new());
    charge(st);
    if st.frames.last().is_some_and(|f| f.kind == FrameKind::Job) {
        st.frames.pop();
    }
    set_mode_for_top(st);
}

pub(super) fn entry_body() {
    let _input_logging = input_record::Scope::enter();
    let st = state();
    if st.entry_body_seq == 0 {
        st.seq += 1;
        st.entry_body_seq = st.seq;
        // The entry's own body runs at startup in every run: a pseudo-init in
        // D (design §2.10.7), so what it writes and reads is D's, and the
        // inits it reaches (its `import()`) are checked against it.
        charge(st);
        let id = st.inits.len() as u32;
        let mut init = blank_init(ENTRY_BODY_ADDR, 0, Reach::EntryEager);
        init.enter_seq = st.seq;
        init.open = true;
        init.in_d = true;
        init.from_entry_body = true;
        init.host = Some(Hit {
            class: Effect::HostRead as u8,
            what: "entry_body",
            pcs: [0; 6],
        });
        st.inits.push(init);
        st.entry_init = id;
        input_record::event(st, "entry_body", String::new());
        st.frames.push(Frame {
            init: id,
            kind: FrameKind::Init,
        });
        set_mode_for_top(st);
    }
}

/// The entry module's evaluation is next (`js_snap_entry_module`).
pub(super) fn entry_module() {
    state().entry_module_next = true;
}

/// `object_meta_ensure` stores a fresh (all-zero) record into the owner's
/// meta word at `slot`: an empty meta behaves as no meta, so the store is
/// representation, like the string alias hint (design §2.10.11).
pub(super) fn meta_install(slot: usize) {
    let _input_logging = input_record::Scope::enter();
    let st = state();
    if st.meta_rule {
        st.meta_slot = slot & !7;
    }
}

/// The record `meta` now hangs off the owner's meta word `slot`. What is
/// written into it later is a write of the owner: under rule 4 the record
/// of a pre-existing owner is pre-existing too.
pub(super) fn meta_installed(slot: usize, meta: usize) {
    let _input_logging = input_record::Scope::enter();
    let st = state();
    st.meta_slot = 0;
    st.meta_installs += 1;
    if !st.meta_rule || !st.rule4_armed || !pre_existing(st, slot) {
        return;
    }
    let Some(h) = object_start(st, meta) else {
        return;
    };
    let Some(size) = st.sizes.get(h as u64) else {
        return;
    };
    let e = h + size as usize;
    let i = st.pre_sorted.partition_point(|&(a, _)| a <= h);
    st.pre_sorted.insert(i, (h, e));
    st.meta_owned += 1;
    let mut p = h & !(PAGE - 1);
    while p < e {
        flag_page(st, p, PF_PRE);
        unsafe { libc::mprotect(p as *mut _, PAGE, policy(st, p)) };
        p += PAGE;
    }
}

/// A classification mark (`ensure_meta_for_mark`: the prototype or exotic
/// read receiver flag, its empty record, the private shape lineage) starts
/// (`delta` 1) or ends (-1): representation, not program state. Returns
/// whether the scope is active (the meta rule is on).
/// A keyed operation on `obj` names `key` (§2.10.13): when `obj` is the
/// realm global, the census takes accesses of its shape word as accesses of
/// `key` until [`global_key_end`]. Returns the previous key to restore.
pub(super) fn global_key<'a>(
    obj: usize,
    key: impl FnOnce() -> Option<&'a [u8]>,
) -> Option<(u64, [u8; GKEY_NAME], u8)> {
    let _input_logging = input_record::Scope::enter();
    let st = state();
    if !st.gkey_rule {
        return None;
    }
    if st.gshape == 0 {
        st.gshape = crate::object::global_this::thread_realm_global_addr();
    }
    if obj == 0 || obj != st.gshape {
        return None;
    }
    let prev = (st.gkey, st.gkey_name, st.gkey_len);
    // The key's bytes may sit on a tracked page: reading them is census
    // bookkeeping, not an access of the running init.
    st.bookkeeping += 1;
    let named = key().map(|key| {
        st.gkey = gkey_loc(key);
        let n = key.len().min(GKEY_NAME);
        st.gkey_name[..n].copy_from_slice(&key[..n]);
        st.gkey_len = n as u8;
    });
    st.bookkeeping -= 1;
    // A key the operation cannot name leaves the access unkeyed.
    named.map(|()| prev)
}

/// The keyed operation [`global_key`] opened is over.
pub(super) fn global_key_end(prev: (u64, [u8; GKEY_NAME], u8)) {
    let _input_logging = input_record::Scope::enter();
    let st = state();
    (st.gkey, st.gkey_name, st.gkey_len) = prev;
}

pub(super) fn representation(delta: i32) -> bool {
    let _input_logging = input_record::Scope::enter();
    let st = state();
    if !st.meta_rule {
        return false;
    }
    if delta > 0 {
        st.marks += 1;
    }
    st.bookkeeping += delta;
    st.repr_depth += delta;
    true
}

pub(super) fn attached(obj: usize, cells: &[usize]) {
    let _input_logging = input_record::Scope::enter();
    let st = state();
    if st.transparent.is_empty() {
        return;
    }
    let Some(h) = object_start(st, obj) else {
        return;
    };
    if !st.transp.contains(h as u64) {
        return;
    }
    for &c in cells {
        if let Some(ch) = object_start(st, c) {
            st.transp.insert(ch as u64);
            st.transp_no.remove(ch as u64);
            if let Some(s) = st.sizes.get(ch as u64) {
                st.transp_objs.push((ch, ch + s as usize));
            }
        }
    }
}

pub(super) fn dyn_code(kind: u8) {
    let _input_logging = input_record::Scope::enter();
    let st = state();
    let top = st.frames.last().map_or(0, |f| f.init) as usize;
    if top == 0 {
        return;
    }
    if kind == 0 {
        st.inits[top].dyn_fns += 1;
    } else {
        st.inits[top].evals += 1;
    }
    if st.dyn_defer {
        host(
            Effect::HostWrite,
            if kind == 0 { "new_function" } else { "eval" },
        );
    }
}

pub(super) fn input_scope(delta: i32) {
    let _input_logging = input_record::Scope::enter();
    let st = state();
    if delta > 0 {
        st.inputs.call_depth += 1;
    } else {
        st.inputs.call_depth -= 1;
    }
    set_mode_for_top(st);
}

pub(super) fn input_event(op: &str, value: String) -> u64 {
    let _input_logging = input_record::Scope::enter();
    input_record::event(state(), op, value)
}

pub(super) fn host(class: Effect, what: &'static str) {
    let _input_logging = input_record::Scope::enter();
    let st = state();
    let input_top = top_init(st) as usize;
    if class != Effect::TargetFact
        && st.inputs.call_depth == 0
        && input_top != 0
        && st.inputs.writes.get(input_top).is_some_and(Option::is_none)
    {
        input_record::remember_write(st, 0, 0);
        st.inputs.watching = false;
        unsafe {
            apply_protection(st);
        }
    }

    input_record::event(st, "effect", format!("{class:?}:{what}"));
    if st.sabotage_env && what == "env" {
        return;
    }
    let top = st.frames.last().map_or(0, |f| f.init);
    if top == 0 {
        st.host_outside += 1;
        if class != Effect::TargetFact && !st.realm_hosts.iter().any(|(w, _)| *w == what) {
            let mut pcs = [0usize; crate::error::MAX_CAPTURED_FRAMES];
            let n = crate::error::capture_ips(&mut pcs);
            let mut six = [0usize; 6];
            for (d, s) in six.iter_mut().zip(pcs[..n].iter().skip(1)) {
                *d = *s;
            }
            st.realm_hosts.push((what, six));
        }
        return;
    }
    let i = &mut st.inits[top as usize];
    i.host_count += 1;
    if class != Effect::TargetFact && !i.in_d {
        i.in_d = true;
        if st.top_mode {
            unsafe { extend_top(st) };
        }
        set_mode_for_top(st);
    }
    let i = &mut st.inits[top as usize];
    if i.host.is_none() || (class != Effect::TargetFact && i.host.is_some_and(|h| h.class == 1)) {
        let mut pcs = [0usize; crate::error::MAX_CAPTURED_FRAMES];
        let n = crate::error::capture_ips(&mut pcs);
        let mut six = [0usize; 6];
        for (d, s) in six.iter_mut().zip(pcs[..n].iter().skip(1)) {
            *d = *s;
        }
        i.host = Some(Hit {
            class: class as u8,
            what,
            pcs: six,
        });
    }
}

fn set_start(st: &mut State, addr: usize) {
    if let Some(i) = start_bit(st, addr) {
        unsafe { *st.starts.add(i / 64) |= 1 << (i % 64) };
    }
}

fn start_bit(st: &State, addr: usize) -> Option<usize> {
    let lo = st.heap.lo;
    (addr >= lo && addr < lo + st.heap.pages * PAGE).then(|| (addr - lo) / 8)
}

/// The birth that holds `word` (its header address), from the start bitmap.
fn object_start(st: &State, word: usize) -> Option<usize> {
    let mut i = start_bit(st, word)?;
    let floor = i.saturating_sub(1 << 22);
    loop {
        let w = unsafe { *st.starts.add(i / 64) } & (u64::MAX >> (63 - (i % 64)));
        if w != 0 {
            let bit = 63 - w.leading_zeros() as usize;
            return Some(st.heap.lo + ((i / 64) * 64 + bit) * 8);
        }
        if i < 64 || i - (i % 64) <= floor {
            return None;
        }
        i = i - (i % 64) - 1;
    }
}

pub(super) fn birth(addr: usize, size: usize) {
    let _input_logging = input_record::Scope::enter();
    let st = state();
    st.inputs.allocated_bytes += size as u64;
    // Memory handed out again (a free-list slot, a rewound bump) holds new
    // locations: an earlier birth there and what members of D did to its
    // words are gone.
    let earlier = object_start(st, addr + size - 1).is_some_and(|h| {
        st.sizes
            .get(h as u64)
            .is_some_and(|s| addr < h + s as usize)
    });
    if earlier {
        st.reborn += 1;
        for w in (addr..addr + size).step_by(8) {
            st.w.remove(w as u64);
            st.r.remove(w as u64);
            st.er.remove(w as u64);
            st.ew.remove(w as u64);
        }
        if let (Some(a), Some(b)) = (start_bit(st, addr), start_bit(st, addr + size - 1)) {
            for i in a..=b {
                unsafe { *st.starts.add(i / 64) &= !(1 << (i % 64)) };
            }
        }
    }
    set_start(st, addr);
    st.sizes.insert(addr as u64, size as u32);
    let top = st.frames.last().map_or(0, |f| f.init);
    if st.inits[top as usize].transparent {
        st.transp.insert(addr as u64);
        st.transp_objs.push((addr, addr + size));
    }
    st.born_by.insert(addr as u64, top);
    st.births.push(Birth {
        addr,
        size: size as u32,
        init: top,
        seq: st.inputs.seq,
    });
    if st.host_what != 0 {
        st.host_targets
            .push((addr, addr + size, st.host_what, st.inputs.seq));
    }
    let i = &mut st.inits[top as usize];
    i.births += 1;
    i.birth_bytes += size as u64;
}

pub(super) fn region_mapped(base: usize, len: usize) {
    let _input_logging = input_record::Scope::enter();
    let st = state();
    if unsafe { libc::syscall(libc::SYS_gettid) } != st.main_tid {
        return;
    }
    st.regions.push((base, base + len));
    mark_tracked(base, base + len);
    if st.d_mode && !st.noprotect {
        unsafe { libc::mprotect(base as *mut _, len, libc::PROT_NONE) };
    }
}

pub(super) fn global_root(slot: usize) {
    let _input_logging = input_record::Scope::enter();
    let st = state();
    // A slot registered again is one address linked into several modules
    // (codegen registers a class-static global in its defining module and in
    // every importer that names it). The registration is GC bookkeeping, not
    // an access: the slot keeps its first owner, and the re-registering init
    // neither initializes it (rule 2) nor owns it (the check run's poison).
    if st.globals.contains(slot as u64) {
        return;
    }
    // The check run: a slot whose first registrant in the census was a member
    // of D (the defining module) arrives poisoned (the census's `.dslots`).
    // That member is skipped here, so the first registration this run sees
    // is an importer's duplicate: bookkeeping as above, and the slot stays
    // D's binding, owned by no imaged init.
    if st.slot_owner_rule && !st.skip.is_empty() && st.env_words.get(slot as u64) == Some(POISON) {
        return;
    }
    count_unwatched_root(st, slot);
    st.globals.insert(slot as u64);
    st.globals_log.push(slot);
    st.slot_owner.push(st.frames.last().map_or(0, |f| f.init));
    if st.inits[st.frames.last().map_or(0, |f| f.init) as usize].transparent {
        st.transp_globals.insert(slot as u64);
    }
    mark_tracked(slot, slot + 8);
    // Rule 2 names every binding a member of D initializes, written or not.
    let top = st.frames.last().map_or(0, |f| f.init) as usize;
    if top != 0 && st.inits[top].in_d {
        st.w.insert(slot as u64);
        st.wby.insert(slot as u64, top as u32, 0);
        flag_page(st, slot & !(PAGE - 1), PF_W);
    }
    let page = slot & !(PAGE - 1);
    if st.d_mode && page_flags(st, page).is_some() {
        unsafe { libc::mprotect(page as *mut _, PAGE, policy(st, page)) };
    }
}

fn mark_tracked(lo: usize, hi: usize) {
    let _input_logging = input_record::Scope::enter();
    let st = state();
    let mut p = lo & !(PAGE - 1);
    while p < hi {
        if let Some(f) = page_flags(st, p) {
            unsafe { *f |= PF_TRACKED };
        }
        p += PAGE;
    }
}

/// A registered root slot on no tracked page: inside the static TLS block it
/// is one of the runtime's own thread-locals (the program's TLS slots all lie
/// on the anchored pages), representation with its own count; anywhere else
/// it is a program slot the census misses.
fn count_unwatched_root(st: &mut State, slot: usize) {
    if page_flags(st, slot).is_some() {
        return;
    }
    if slot >= st.tls_block.0 && slot < st.tls_block.1 {
        st.runtime_tls_roots += 1;
        if st.runtime_tls_first == 0 {
            st.runtime_tls_first = slot;
        }
    } else {
        st.untracked_slots += 1;
        if st.untracked_first == 0 {
            st.untracked_first = slot;
        }
    }
}

/// `PT_TLS` memsz of the executable, rounded up to its alignment.
fn executable_tls_size() -> usize {
    unsafe extern "C" fn each(
        info: *mut libc::dl_phdr_info,
        _size: libc::size_t,
        data: *mut libc::c_void,
    ) -> libc::c_int {
        let info = &*info;
        for i in 0..info.dlpi_phnum as usize {
            let ph = &*info.dlpi_phdr.add(i);
            if ph.p_type == libc::PT_TLS {
                let align = (ph.p_align as usize).max(1);
                *(data as *mut usize) = (ph.p_memsz as usize).div_ceil(align) * align;
            }
        }
        1 // the executable is the first object; stop
    }
    let mut size = 0usize;
    unsafe { libc::dl_iterate_phdr(Some(each), (&mut size as *mut usize).cast()) };
    size
}

#[inline]
fn page_flags(st: &State, addr: usize) -> Option<*mut u8> {
    st.heap
        .slot(addr)
        .or_else(|| st.bin.slot(addr))
        .or_else(|| tls_slot(st, addr))
}

#[inline]
fn tls_slot(st: &State, addr: usize) -> Option<*mut u8> {
    st.tls.slot(addr).or_else(|| st.tlsb.slot(addr))
}

// ---------------------------------------------------------------------------
// Protection
// ---------------------------------------------------------------------------

fn policy(st: &State, page: usize) -> i32 {
    if input_record::watch(st)
        && page_flags(st, page).is_some_and(|f| unsafe { *f & PF_TRACKED != 0 })
    {
        return libc::PROT_READ;
    }
    // The weights run (`PERRY_SNAP_NOPROTECT=1`) takes no fault: its Ir per
    // init is the program's, not the census's.
    if st.noprotect {
        return libc::PROT_READ | libc::PROT_WRITE;
    }
    let Some(f) = page_flags(st, page) else {
        return libc::PROT_READ | libc::PROT_WRITE;
    };
    let f = unsafe { *f };
    if f & PF_TRACKED == 0 {
        return libc::PROT_READ | libc::PROT_WRITE;
    }
    if f & PF_ENV != 0 && !st.noprotect {
        return libc::PROT_NONE;
    }
    if st.top_mode {
        return if !st.d_mode && f & PF_PRE != 0 {
            libc::PROT_NONE
        } else {
            libc::PROT_READ | libc::PROT_WRITE
        };
    }
    if st.d_mode {
        return libc::PROT_NONE;
    }
    if f & PF_W != 0 {
        libc::PROT_NONE
    } else if f & (PF_R | PF_PRE) != 0 {
        libc::PROT_READ
    } else {
        libc::PROT_READ | libc::PROT_WRITE
    }
}

/// Every tracked range: the reservation's regions, the realm section, the
/// pages of global root slots (the binary-range pages flagged tracked).
unsafe fn for_each_tracked_range(st: &State, mut f: impl FnMut(usize, usize)) {
    for &(b, e) in &st.regions {
        f(b, e);
    }
    for m in [&st.bin, &st.tls, &st.tlsb] {
        let mut i = 0;
        while i < m.pages {
            if *m.flags.add(i) & PF_TRACKED != 0 {
                let s = i;
                while i < m.pages && *m.flags.add(i) & PF_TRACKED != 0 {
                    i += 1;
                }
                f(m.lo + s * PAGE, m.lo + i * PAGE);
            } else {
                i += 1;
            }
        }
    }
}

unsafe fn apply_protection(st: &mut State) {
    if input_record::watch(st) {
        for_each_tracked_range(st, |b, e| {
            libc::mprotect(b as *mut _, e - b, libc::PROT_READ);
        });
        return;
    }
    // Ending the first-write watcher restores RW in the weights run,
    // even if a conflict has set d_mode. Bulk protection agrees with policy.
    if st.top_mode || st.noprotect {
        for_each_tracked_range(st, |b, e| {
            libc::mprotect(b as *mut _, e - b, libc::PROT_READ | libc::PROT_WRITE);
        });
        if !st.d_mode {
            for &page in st.listed.as_slice() {
                let p = policy(st, page);
                if p != libc::PROT_READ | libc::PROT_WRITE {
                    libc::mprotect(page as *mut _, PAGE, p);
                }
            }
        }
        return;
    }
    if st.d_mode {
        for_each_tracked_range(st, |b, e| {
            libc::mprotect(b as *mut _, e - b, libc::PROT_NONE);
        });
        return;
    }
    for_each_tracked_range(st, |b, e| {
        libc::mprotect(b as *mut _, e - b, libc::PROT_READ | libc::PROT_WRITE);
    });
    for &page in st.listed.as_slice() {
        let p = policy(st, page);
        if p != libc::PROT_READ | libc::PROT_WRITE {
            libc::mprotect(page as *mut _, PAGE, p);
        }
    }
}

fn flag_page(st: &mut State, page: usize, bit: u8) {
    if let Some(f) = page_flags(st, page) {
        unsafe {
            if *f & PF_LISTED == 0 {
                *f |= PF_LISTED;
                st.listed.push(page);
            }
            *f |= bit;
        }
    }
}

/// Rule 4 (relaxed): the words that exist when the first dynamic import
/// starts are watched for writes by the dynamic subgraph.
unsafe fn arm_rule4(st: &mut State) {
    st.rule4_armed = true;
    st.pre_births = st.births.len;
    let mut v: Vec<(usize, usize)> = st
        .births
        .as_slice()
        .iter()
        .map(|b| (b.addr, b.addr + b.size as usize))
        .collect();
    v.sort_unstable();
    st.pre_sorted = v;
    let pages: Vec<usize> = st
        .pre_sorted
        .iter()
        .flat_map(|&(a, e)| (a & !(PAGE - 1)..e).step_by(PAGE))
        .collect();
    for p in pages {
        flag_page(st, p, PF_PRE);
    }
    let (rb, re) = st.realm;
    let mut p = rb & !(PAGE - 1);
    while p < re {
        flag_page(st, p, PF_PRE);
        p += PAGE;
    }
    let mut globals = Vec::new();
    crate::gc::for_each_global_root_slot(|s| globals.push(s));
    for s in globals {
        st.pre_sorted.push((s, s + 8));
        flag_page(st, s & !(PAGE - 1), PF_PRE);
    }
    st.pre_sorted.sort_unstable();
    apply_protection(st);
}

/// ⊤ mode: everything born or registered so far is pre-existing for the
/// member that starts now.
unsafe fn extend_top(st: &mut State) {
    let from = st.pre_upto;
    st.pre_upto = st.births.len;
    for k in from..st.pre_upto {
        let b = *st.births.p.add(k);
        if let Some(i) = start_bit(st, b.addr) {
            *st.pre_bits.add(i / 64) |= 1 << (i % 64);
        }
        let mut p = b.addr & !(PAGE - 1);
        while p < b.addr + b.size as usize {
            flag_page(st, p, PF_PRE);
            p += PAGE;
        }
    }
    for k in st.pre_globals_upto..st.globals_log.len() {
        let s = st.globals_log[k];
        st.pre_globals.insert(s as u64);
        flag_page(st, s & !(PAGE - 1), PF_PRE);
    }
    st.pre_globals_upto = st.globals_log.len();
    if !st.top_armed {
        st.top_armed = true;
        let (rb, re) = st.realm;
        let mut p = rb & !(PAGE - 1);
        while p < re {
            flag_page(st, p, PF_PRE);
            p += PAGE;
        }
    }
}

fn top_pre_existing(st: &State, word: usize) -> bool {
    if !st.top_armed {
        return false;
    }
    if word >= st.realm.0 && word < st.realm.1 {
        return true;
    }
    if st.pre_globals.contains(word as u64) {
        return true;
    }
    match object_start(st, word).and_then(|h| start_bit(st, h)) {
        Some(i) => unsafe { *st.pre_bits.add(i / 64) & (1 << (i % 64)) != 0 },
        None => false,
    }
}

fn pre_existing(st: &State, word: usize) -> bool {
    let (rb, re) = st.realm;
    if word >= rb && word < re {
        return true;
    }
    let i = st.pre_sorted.partition_point(|&(a, _)| a <= word);
    i > 0 && word < st.pre_sorted[i - 1].1
}

// ---------------------------------------------------------------------------
// Signals
// ---------------------------------------------------------------------------

/// The dispositions the runner replaced, alone on their page (a page of
/// `.bss` may also hold module globals, which the runner protects).
#[repr(C, align(4096))]
struct Prev(std::mem::MaybeUninit<libc::sigaction>);
static mut PREV_SEGV: Prev = Prev(std::mem::MaybeUninit::uninit());
static mut PREV_TRAP: Prev = Prev(std::mem::MaybeUninit::uninit());

unsafe fn install_handlers() {
    // The handlers run on their own stack: a fault whose frame cannot be
    // pushed on the current stack (a protected page) still reaches them.
    let size = 1 << 20;
    let ss = libc::stack_t {
        ss_sp: map_zeroed::<u8>(size).cast(),
        ss_flags: 0,
        ss_size: size,
    };
    libc::sigaltstack(&ss, std::ptr::null_mut());
    let mut a: libc::sigaction = std::mem::zeroed();
    a.sa_sigaction = on_segv as *const () as usize;
    a.sa_flags = libc::SA_SIGINFO | libc::SA_NODEFER | libc::SA_ONSTACK;
    libc::sigemptyset(&mut a.sa_mask);
    libc::sigaction(
        libc::SIGSEGV,
        &a,
        std::ptr::addr_of_mut!(PREV_SEGV.0).cast(),
    );
    let mut t: libc::sigaction = std::mem::zeroed();
    t.sa_sigaction = on_trap as *const () as usize;
    t.sa_flags = libc::SA_SIGINFO | libc::SA_ONSTACK;
    libc::sigemptyset(&mut t.sa_mask);
    libc::sigaction(
        libc::SIGTRAP,
        &t,
        std::ptr::addr_of_mut!(PREV_TRAP.0).cast(),
    );
}

/// One stderr line of hex words, without allocating (signal context).
unsafe fn diag(words: &[u64]) {
    let mut buf = [0u8; 256];
    let mut n = 0;
    for s in b"snap census: foreign fault" {
        buf[n] = *s;
        n += 1;
    }
    for &w in words {
        buf[n] = b' ';
        n += 1;
        for i in (0..16).rev() {
            buf[n] = b"0123456789abcdef"[((w >> (i * 4)) & 15) as usize];
            n += 1;
        }
    }
    buf[n] = b'\n';
    libc::write(2, buf.as_ptr().cast(), n + 1);
}

unsafe fn chain(prev: *const libc::sigaction, sig: i32) {
    // The check run after a poisoned read: the program ran on with a skipped
    // member's binding, so a later fault is that read's consequence. The
    // `#checkfail` lines are in `<out>.checkfail` already; end with them.
    let st_ptr = ANCHOR.state.load(Relaxed);
    if sig == libc::SIGSEGV && !st_ptr.is_null() && !(*st_ptr).check_fail.is_empty() {
        let m = b"snap census: check run fault after a poisoned read (see .checkfail)\n";
        libc::write(2, m.as_ptr().cast(), m.len());
        libc::_exit(3);
    }
    // Not ours: restore the previous disposition; the instruction faults
    // again and takes it.
    libc::sigaction(sig, prev, std::ptr::null_mut());
}

const EFLAGS_TF: i64 = 0x100;

extern "C" fn on_segv(sig: libc::c_int, info: *mut libc::siginfo_t, ctx: *mut libc::c_void) {
    unsafe {
        let addr = (*info).si_addr() as usize;
        let st_ptr = ANCHOR.state.load(Relaxed);
        if st_ptr.is_null() {
            return chain(std::ptr::addr_of!(PREV_SEGV.0).cast(), sig);
        }
        let st = &mut *st_ptr;
        let tid = libc::syscall(libc::SYS_gettid);
        if tid != st.main_tid {
            let page = addr & !(PAGE - 1);
            let tracked = page_flags(st, page).is_some_and(|f| *f & PF_TRACKED != 0);
            let free = st.foreign.iter().position(|e| e.0 == 0);
            if !tracked || free.is_none() {
                let uc = ctx as *mut libc::ucontext_t;
                let pc = (*uc).uc_mcontext.gregs[libc::REG_RIP as usize] as u64;
                diag(&[0x7d, tid as u64, addr as u64, pc]);
                return chain(std::ptr::addr_of!(PREV_SEGV.0).cast(), sig);
            }
            st.foreign_faults += 1;
            st.foreign[free.unwrap()] = (tid, page);
            libc::mprotect(page as *mut _, PAGE, libc::PROT_READ | libc::PROT_WRITE);
            let uc = ctx as *mut libc::ucontext_t;
            (*uc).uc_mcontext.gregs[libc::REG_EFL as usize] |= EFLAGS_TF;
            return;
        }
        let _input_logging = input_record::Scope::enter();
        if st.in_handler > 0 {
            let uc = ctx as *mut libc::ucontext_t;
            let pc = (*uc).uc_mcontext.gregs[libc::REG_RIP as usize] as u64;
            let top = st.frames.last().map_or(0, |f| f.init) as u64;
            diag(&[0xdead, addr as u64, pc, top]);
            return chain(std::ptr::addr_of!(PREV_SEGV.0).cast(), sig);
        }
        st.in_handler += 1;
        on_segv_inner(st, sig, info, ctx);
        st.in_handler -= 1;
    }
}

unsafe fn on_segv_inner(
    st: &mut State,
    sig: libc::c_int,
    info: *mut libc::siginfo_t,
    ctx: *mut libc::c_void,
) {
    {
        let addr = (*info).si_addr() as usize;
        let page = addr & !(PAGE - 1);
        if st.rset_active {
            let uc = ctx as *mut libc::ucontext_t;
            let gregs = &mut (*uc).uc_mcontext.gregs;
            if gregs[libc::REG_ERR as usize] & 2 == 0 || st.heap.slot(addr).is_none() {
                return chain(std::ptr::addr_of!(PREV_SEGV.0).cast(), sig);
            }
            st.rset_faults += 1;
            let word = addr & !7;
            if let Some(i) = cand_index(st, word) {
                st.cand_writes[i] = st.cand_writes[i].saturating_add(1);
                st.rset_word = word | 1;
            }
            if let Some(slot) = st.pending.iter_mut().find(|s| **s == 0) {
                *slot = page;
            }
            libc::mprotect(page as *mut _, PAGE, libc::PROT_READ | libc::PROT_WRITE);
            gregs[libc::REG_EFL as usize] |= EFLAGS_TF;
            return;
        }
        let tracked = page_flags(st, page).is_some_and(|f| *f & PF_TRACKED != 0);
        // A tracked page whose policy is open now but whose protection is
        // still a bulk protect of an earlier mode (the watch ended while the
        // page stayed read-only): bring the page to its policy and retry. An
        // extra closed page only costs a fault; it never hides a write.
        if tracked && policy(st, page) == libc::PROT_READ | libc::PROT_WRITE {
            libc::mprotect(page as *mut _, PAGE, libc::PROT_READ | libc::PROT_WRITE);
            st.faults_ignored += 1;
            return;
        }
        if !tracked {
            let uc = ctx as *mut libc::ucontext_t;
            let pc = (*uc).uc_mcontext.gregs[libc::REG_RIP as usize] as u64;
            let flags = page_flags(st, page).map_or(0xff, |f| *f as u64);
            let range = if st.heap.slot(addr).is_some() {
                1
            } else if st.bin.slot(addr).is_some() {
                2
            } else if tls_slot(st, addr).is_some() {
                3
            } else {
                0
            };
            diag(&[
                addr as u64,
                pc,
                flags,
                range,
                st.d_mode as u64,
                st.top_mode as u64,
            ]);
            return chain(std::ptr::addr_of!(PREV_SEGV.0).cast(), sig);
        }
        let uc = ctx as *mut libc::ucontext_t;
        let gregs = &mut (*uc).uc_mcontext.gregs;
        let write = gregs[libc::REG_ERR as usize] & 2 != 0;
        let pc = gregs[libc::REG_RIP as usize] as usize;
        st.faults_total += 1;
        // Open the page for one instruction (before recording: the record
        // may read an object header on it).
        if let Some(slot) = st.pending.iter_mut().find(|s| **s == 0) {
            *slot = page;
        }
        libc::mprotect(page as *mut _, PAGE, libc::PROT_READ | libc::PROT_WRITE);
        let word = addr & !7;
        match st.stores.iter_mut().find(|s| s.0 == 0) {
            Some(slot) if write => *slot = (word, *(word as *const u64), pc),
            _ => {
                if note_access(st, word, write, pc) {
                    end_watch(st);
                }
            }
        }
        gregs[libc::REG_EFL as usize] |= EFLAGS_TF;
    }
}

extern "C" fn on_trap(sig: libc::c_int, _info: *mut libc::siginfo_t, ctx: *mut libc::c_void) {
    unsafe {
        let st_ptr = ANCHOR.state.load(Relaxed);
        if !st_ptr.is_null() {
            let st = &mut *st_ptr;
            let tid = libc::syscall(libc::SYS_gettid);
            if tid != st.main_tid {
                let mut mine = false;
                for k in 0..st.foreign.len() {
                    if st.foreign[k].0 == tid {
                        let page = st.foreign[k].1;
                        st.foreign[k] = (0, 0);
                        libc::mprotect(page as *mut _, PAGE, policy(st, page));
                        mine = true;
                    }
                }
                if mine {
                    let uc = ctx as *mut libc::ucontext_t;
                    (*uc).uc_mcontext.gregs[libc::REG_EFL as usize] &= !EFLAGS_TF;
                    return;
                }
                return chain(std::ptr::addr_of!(PREV_TRAP.0).cast(), sig);
            }
        }
        if st_ptr.is_null() || (*st_ptr).pending[0] == 0 {
            return chain(std::ptr::addr_of!(PREV_TRAP.0).cast(), sig);
        }
        let _input_logging = input_record::Scope::enter();
        let st = &mut *st_ptr;
        // A fault in this handler (SIGTRAP is blocked here) must not open a
        // page and set TF: the single step would be a blocked SIGTRAP, which
        // kills the process silently. on_segv reports it instead (0xdead).
        st.in_handler += 1;
        let pending = st.pending;
        if st.rset_active {
            let w = std::mem::replace(&mut st.rset_word, 0);
            if w != 0 {
                let word = w & !1;
                if heap_pointer(st, *(word as *const u64)) {
                    if let Some(i) = cand_index(st, word) {
                        st.cand_ptr_writes[i] = st.cand_ptr_writes[i].saturating_add(1);
                    }
                }
            }
            for &page in pending.iter().take_while(|p| **p != 0) {
                libc::mprotect(page as *mut _, PAGE, libc::PROT_READ);
            }
        } else {
            // A store that left the word's value as it was is no write
            // (nothing anyone reads changed); it still read the word.
            let stores = std::mem::replace(&mut st.stores, [(0, 0, 0); 4]);
            let mut watch_ended = false;
            for &(word, before, pc) in stores.iter().take_while(|s| s.0 != 0) {
                let changed = *(word as *const u64) != before;
                if !changed {
                    st.silent_stores += 1;
                }
                st.store_before = Some(before);
                watch_ended |= note_access(st, word, changed, pc);
                st.store_before = None;
            }
            if watch_ended {
                apply_protection(st);
            }
            for &page in pending.iter().take_while(|p| **p != 0) {
                libc::mprotect(page as *mut _, PAGE, policy(st, page));
            }
        }
        st.pending = [0; 8];
        st.in_handler -= 1;
        let uc = ctx as *mut libc::ucontext_t;
        (*uc).uc_mcontext.gregs[libc::REG_EFL as usize] &= !EFLAGS_TF;
    }
}

/// One access to a tracked word by the top frame.
/// A word on any page, whatever its protection.
unsafe fn peek(st: &State, word: usize) -> u64 {
    let page = word & !(PAGE - 1);
    let closed = policy(st, page) & libc::PROT_READ == 0;
    if closed {
        libc::mprotect(page as *mut _, PAGE, libc::PROT_READ);
    }
    let v = *(word as *const u64);
    if closed {
        libc::mprotect(page as *mut _, PAGE, policy(st, page));
    }
    v
}

/// `h` is born by a transparent init, or is the overflow table or meta cell
/// one of those objects points at (decided once per cell).
fn transparent_cell(st: &mut State, h: usize, ty: u8) -> bool {
    if st.transp.contains(h as u64) {
        return true;
    }
    if (ty != crate::gc::GC_TYPE_OVERFLOW && ty != crate::gc::GC_TYPE_OBJECT_META)
        || st.transp_no.contains(h as u64)
    {
        return false;
    }
    let user = (h + crate::gc::GC_HEADER_SIZE) as u64;
    let found = st.transp_objs.iter().any(|&(a, e)| {
        (a..e).step_by(8).any(|w| {
            let v = unsafe { peek(st, w) };
            v == user || v == h as u64
        })
    });
    if found {
        st.transp.insert(h as u64);
    } else {
        st.transp_no.insert(h as u64);
    }
    found
}

/// True when this access ended the first-write watch: the caller re-applies
/// the bulk protection once the fault is recorded (`end_watch`).
#[must_use]
fn note_access(st: &mut State, word: usize, write: bool, pc: usize) -> bool {
    let mut watch_ended = false;
    note_access_at(st, word, write, pc, &mut watch_ended);
    watch_ended
}

/// The end of the first-write watch re-protects every tracked page. It runs
/// after the fault's records: they read the stored words and cell headers on
/// the open pages, and a fault inside on_trap is a blocked SIGTRAP, which
/// kills the process.
unsafe fn end_watch(st: &mut State) {
    apply_protection(st);
    // The pages of the instruction being stepped stay open until on_trap
    // restores their policy.
    for &page in st.pending.iter().take_while(|p| **p != 0) {
        libc::mprotect(page as *mut _, PAGE, libc::PROT_READ | libc::PROT_WRITE);
    }
}

fn note_access_at(st: &mut State, word: usize, write: bool, pc: usize, watch_ended: &mut bool) {
    if st.r1 {
        // The census's own bookkeeping (a root registration, a key's bytes)
        // is no access; a representation scope or a meta install is one, of
        // representation (R1).
        if st.bookkeeping > st.repr_depth {
            st.faults_ignored += 1;
            return;
        }
        if st.repr_depth > 0 || (write && word == st.meta_slot) {
            repr_access(st, word, write, pc);
            return;
        }
    }
    if st.bookkeeping > 0 {
        st.faults_ignored += 1;
        return;
    }
    if write && word == st.meta_slot {
        st.faults_ignored += 1;
        return;
    }
    if st.env_words.len > 0 {
        if let Some(v) = st.env_words.get(word as u64) {
            env_word_access(st, v, word, write, pc);
            return;
        }
    }
    // Words of the binary range count only as global slots or realm cells.
    if (st.bin.slot(word).is_some() || tls_slot(st, word).is_some())
        && !(word >= st.realm.0 && word < st.realm.1)
        && !st.globals.contains(word as u64)
    {
        st.faults_ignored += 1;
        return;
    }
    if write && input_record::watch(st) {
        input_record::changed_word(st, word, pc);
        if !input_record::watch(st) {
            st.inputs.watching = false;
            *watch_ended = true;
        }
    }
    if st.transp_globals.contains(word as u64) {
        st.transparent_hits += 1;
        return;
    }
    // Immutable value-identity cells are not locations: a string never
    // changes after its birth (its cached hash is no observation), and a
    // shared canonical key list is an interned trie node. Reaching one
    // through a location a member of D wrote is caught at that location.
    let cell = if st.heap.slot(word).is_some() {
        object_start(st, word)
    } else {
        realm_cell(st, word)
    };
    if st.heap.slot(word).is_some() || cell.is_some() {
        // A word outside every born object is free space the allocator
        // touched (block setup), not a location.
        let inside = cell.is_some_and(|h| {
            st.sizes
                .get(h as u64)
                .is_some_and(|s| word < h + s as usize)
        });
        if !inside {
            st.free_space += 1;
            return;
        }
        if let Some(h) = cell {
            let hp = h & !(PAGE - 1);
            let fp = word & !(PAGE - 1);
            let (ty, flags) = unsafe {
                if hp != fp {
                    libc::mprotect(hp as *mut _, PAGE, libc::PROT_READ);
                }
                let g = &*(h as *const crate::gc::GcHeader);
                let r = (g.obj_type, g.gc_flags);
                if hp != fp {
                    libc::mprotect(hp as *mut _, PAGE, policy(st, hp));
                }
                r
            };
            if ty == crate::gc::GC_TYPE_STRING || flags & crate::gc::GC_FLAG_SHAPE_SHARED != 0 {
                st.excluded += 1;
                // R1: a member of D changing an excluded cell in place (an
                // in-place append) another init bore: rolled back.
                if st.r1 && write && st.d_mode && !born_in_d(st, h) {
                    note_repw(st, word);
                }
                return;
            }
            if !st.transparent.is_empty() && transparent_cell(st, h, ty) {
                st.transparent_hits += 1;
                return;
            }
        }
    }
    let top = st.frames.last().map_or(0, |f| f.init) as usize;
    st.inits[top].faults += 1;
    if st.gkey_rule && st.gshape == 0 {
        st.gshape = crate::object::global_this::thread_realm_global_addr();
    }
    if st.gkey_rule && word == st.gshape && st.gshape != 0 {
        // §2.10.13: the realm global's shape word is the global's key set.
        let key = st.gkey;
        let (kind, locs): (usize, [(u64, bool); 2]) = match (key != 0, write) {
            (true, false) => (0, [(key, false), (GKEY_UNKEYED, false)]),
            (true, true) => (1, [(key, true), (GKEY_ANY, true)]),
            (false, false) => (2, [(GKEY_ANY, false), (GKEY_ANY, false)]),
            (false, true) => (3, [(GKEY_ANY, true), (GKEY_UNKEYED, true)]),
        };
        st.gkey_counts[kind] += 1;
        let had = st.inits[top].conflict.is_some();
        for (loc, w) in locs {
            note_loc(st, top, word, loc, w, pc);
        }
        if !had
            && key != 0
            && st.inits[top].conflict.is_some()
            && st.gkey_conflicts.len() < GKEY_CONFLICTS_MAX
        {
            // Capacity is reserved: no allocation in the fault handler.
            let e = (top as u32, key, st.gkey_name, st.gkey_len);
            st.gkey_conflicts.push(e);
        }
        return;
    }
    note_loc(st, top, word, word as u64, write, pc);
}

/// A rolled-back word (R1): the image holds it as before D, so its value
/// before this (the first) write is kept for the digest's content names.
fn note_repw(st: &mut State, word: usize) {
    st.repw_writes += 1;
    if st.repw.insert(word as u64) {
        // A store classified after its step names the value before it; at
        // the fault the word still holds it (its page is open).
        let old = st
            .store_before
            .unwrap_or_else(|| unsafe { *(word as *const u64) });
        st.repw_old.push((word as u64, old));
    }
}

/// The object starting at `h` was born while a member of D ran.
fn born_in_d(st: &State, h: usize) -> bool {
    st.born_by
        .get(h as u64)
        .is_some_and(|i| i != 0 && st.inits[i as usize].in_d)
}

/// R1 (design §2.10.16): an access inside a representation scope or a meta
/// install. Representation decides ordering, never who owns a write. A
/// member of D writing an object it (or another member) bore writes D state
/// (`st.w`): imaged inits that later touch those words are rule 2/3
/// readers. A member of D writing any other object's word: the word joins
/// the rollback set (`repw`, the digest and `.wd` leave it out), no edge.
/// An imaged init's representation access is an access only of D state.
fn repr_access(st: &mut State, word: usize, write: bool, pc: usize) {
    let cell = if st.heap.slot(word).is_some() {
        object_start(st, word)
    } else {
        realm_cell(st, word)
    };
    let Some(h) = cell.filter(|&h| {
        st.sizes
            .get(h as u64)
            .is_some_and(|s| word < h + s as usize)
    }) else {
        st.faults_ignored += 1;
        return;
    };
    // A string or a shared canonical key list is a value (R2): a member of
    // D extending one another init bore in place is rolled back; no other
    // access of it is an access.
    // The faulting page is open for the access; another header page is
    // opened for the read only (as in `note_access`).
    let (ty, flags) = unsafe {
        let hp = h & !(PAGE - 1);
        if hp != word & !(PAGE - 1) {
            libc::mprotect(hp as *mut _, PAGE, libc::PROT_READ);
        }
        let g = &*(h as *const crate::gc::GcHeader);
        let r = (g.obj_type, g.gc_flags);
        if hp != word & !(PAGE - 1) {
            libc::mprotect(hp as *mut _, PAGE, policy(st, hp));
        }
        r
    };
    if ty == crate::gc::GC_TYPE_STRING || flags & crate::gc::GC_FLAG_SHAPE_SHARED != 0 {
        st.excluded += 1;
        if st.d_mode && write && !born_in_d(st, h) {
            note_repw(st, word);
        }
        return;
    }
    let top = st.frames.last().map_or(0, |f| f.init) as usize;
    if st.d_mode {
        if !write {
            st.faults_ignored += 1;
        } else if born_in_d(st, h) {
            st.repw_owned += 1;
            st.inits[top].faults += 1;
            note_loc(st, top, word, word as u64, true, pc);
        } else {
            note_repw(st, word);
        }
        return;
    }
    if top != 0 && st.w.contains(word as u64) {
        st.repw_reads += 1;
        st.inits[top].faults += 1;
        note_loc(st, top, word, word as u64, write, pc);
        return;
    }
    st.faults_ignored += 1;
}

/// One access of location `w` (a word, or a key id of the realm global whose
/// shape word is `word`) by init `top`: rules 2-4.
fn note_loc(st: &mut State, top: usize, word: usize, w: u64, write: bool, pc: usize) {
    if st.top_mode {
        if st.d_mode || top == 0 || !top_pre_existing(st, word) {
            return;
        }
        let i = &mut st.inits[top];
        if i.conflict.is_none() {
            i.conflict = Some(Conflict {
                rule: if write { 3 } else { 2 },
                word,
                pc,
                write,
                cause: 0,
                cause_pc: 0,
            });
        }
        i.in_d = true;
        unsafe { extend_top(st) };
        st.d_mode = true;
        unsafe { apply_protection(st) };
        return;
    }
    if st.p_entry_rule && top != 0 {
        if is_entry(st, top as u32) {
            if write {
                st.ew.insert(w);
            } else {
                st.er.insert(w);
            }
        } else {
            let rule = if st.inits[top].p_entry_hit {
                None
            } else if write {
                st.er.contains(w).then_some(b'b')
            } else {
                st.ew.contains(w).then_some(b'a')
            };
            if write {
                // The word's last writer is no longer an entry.
                st.ew.remove(w);
            }
            if let Some(rule) = rule {
                if st.p_entry_log.len() < st.p_entry_log.capacity() {
                    st.inits[top].p_entry_hit = true;
                    let seq = st.inputs.seq;
                    st.p_entry_log.push((top as u32, rule, word, pc, seq));
                }
            }
        }
    }
    if st.d_mode {
        // A member (seeded, or joined earlier in this run) still names the
        // first earlier member it conflicts with: the edge of the cascade
        // tree for an init the seed put in D from its start.
        if top != 0 && st.inits[top].conflict.is_none() {
            let hit = if st.w.contains(w) {
                st.wby.get(w).map(|c| (if write { 3 } else { 2 }, c))
            } else if write && st.r.contains(w) {
                st.rby.get(w).map(|c| (3, c))
            } else {
                None
            };
            if let Some((rule, (cause, cause_pc))) = hit.filter(|h| h.1 .0 != top as u32) {
                st.inits[top].conflict = Some(Conflict {
                    rule,
                    word,
                    pc,
                    write,
                    cause,
                    cause_pc,
                });
            }
        }
        if write {
            st.w.insert(w);
            st.wby.insert(w, top as u32, pc);
            flag_page(st, word & !(PAGE - 1), PF_W);
        } else if !st.w.contains(w) {
            if st.r.insert(w) {
                st.rby.insert(w, top as u32, pc);
            }
            flag_page(st, word & !(PAGE - 1), PF_R);
        }
        return;
    }
    if top == 0 {
        return;
    }
    let rule = if st.w.contains(w) {
        Some(if write { 3 } else { 2 })
    } else if write && st.r.contains(w) {
        Some(3)
    } else if write && st.rule4_armed && st.inits[top].dynamic && pre_existing(st, word) {
        Some(4)
    } else {
        None
    };
    if let Some(rule) = rule {
        let (cause, cause_pc) = match rule {
            2 => st.wby.get(w),
            3 if st.w.contains(w) => st.wby.get(w),
            3 => st.rby.get(w),
            _ => None,
        }
        .unwrap_or((0, 0));
        let i = &mut st.inits[top];
        if i.conflict.is_none() {
            i.conflict = Some(Conflict {
                rule,
                word,
                pc,
                write,
                cause,
                cause_pc,
            });
        }
        i.in_d = true;
        st.d_mode = true;
        unsafe { apply_protection(st) };
        // This access is the member's first recorded one.
        if write {
            st.w.insert(w);
            st.wby.insert(w, top as u32, pc);
            flag_page(st, word & !(PAGE - 1), PF_W);
        } else {
            if st.r.insert(w) {
                st.rby.insert(w, top as u32, pc);
            }
            flag_page(st, word & !(PAGE - 1), PF_R);
        }
    }
}

// ---------------------------------------------------------------------------
// Ground truth: getenv
// ---------------------------------------------------------------------------

unsafe extern "C" {
    static mut environ: *const *const libc::c_char;
}

/// The process's `getenv`, interposed in the instrumented archives so the
/// census sees every environment read by any code (Rust std, the runtime's
/// knobs, C libraries), not only the entries that declare `env`.
#[no_mangle]
pub unsafe extern "C" fn getenv(name: *const libc::c_char) -> *mut libc::c_char {
    let saved_errno = *libc::__errno_location();
    if name.is_null() {
        return std::ptr::null_mut();
    }
    let key = std::ffi::CStr::from_ptr(name).to_bytes();
    let mut found: *mut libc::c_char = std::ptr::null_mut();
    let mut e = environ;
    if !e.is_null() {
        while !(*e).is_null() {
            let entry = std::ffi::CStr::from_ptr(*e).to_bytes();
            if entry.len() > key.len() && entry.starts_with(key) && entry[key.len()] == b'=' {
                found = (*e).add(key.len() + 1) as *mut libc::c_char;
                break;
            }
            e = e.add(1);
        }
    }
    if inputs_on() && on_main_thread() {
        let _input_logging = input_record::Scope::enter();
        let st = state();
        let op = if c25() && FUNNEL.get() == 7 {
            "getenv.program"
        } else {
            "getenv"
        };
        if found.is_null() {
            input_record::native_event(st, op, format_args!("{}=unset", input_record::Hex(key)));
        } else {
            input_record::native_event(
                st,
                op,
                format_args!(
                    "{}={}",
                    input_record::Hex(key),
                    input_record::Hex(std::ffi::CStr::from_ptr(found).to_bytes())
                ),
            );
        }
    }
    if inputs_on() && !on_main_thread() {
        let value = if found.is_null() {
            b"<unset>".as_slice()
        } else {
            std::ffi::CStr::from_ptr(found).to_bytes()
        };
        foreign_input(b"getenv", key, value);
    }
    *libc::__errno_location() = saved_errno;
    found
}

// ---------------------------------------------------------------------------
// Boundary and report
// ---------------------------------------------------------------------------

/// True when the collection must not run (R5 keeps the heap in place).
pub(super) fn skip_collection() -> bool {
    let p = ANCHOR.state.load(Relaxed);
    !p.is_null() && unsafe { (*p).rset_active }
}

pub(super) fn boundary(at: &'static str) {
    let _input_logging = input_record::Scope::enter();
    let st = state();
    if st.boundary_seq != 0 {
        return;
    }
    input_record::event(st, "boundary", at.to_owned());
    charge(st);
    st.seq += 1;
    st.boundary_seq = st.seq;
    st.boundary_at = at;
    ANCHOR.on.store(false, Relaxed);
    st.d_mode = false;
    unsafe {
        for_each_tracked_range(st, |b, e| {
            libc::mprotect(b as *mut _, e - b, libc::PROT_READ | libc::PROT_WRITE);
        });
    }
    crate::arena::refresh_inline_allocation_limit();
    write_report(st);
    input_record::write(st).expect("write ordered input record");
    // The check run proves the image at the boundary; what runs after it
    // reads bindings of the skipped members of D, so the run ends here.
    if !st.skip.is_empty() {
        unsafe { libc::_exit(0) };
    }
    if st.rset {
        unsafe { arm_rset(st) };
        return;
    }
    crate::gc::gc_unsuppress();
    // Births sorted by address for the census attribution.
    st.births.as_mut_slice().sort_unstable_by_key(|b| b.addr);
}

/// R5: the imaged cells (births of inits outside D, the runtime's included)
/// are write-protected for the rest of the run; collection stays off so they
/// keep their addresses; new births go to fresh blocks.
unsafe fn arm_rset(st: &mut State) {
    let mut cand: Vec<(usize, usize)> = st
        .births
        .as_slice()
        .iter()
        .filter(|b| b.init == 0 || !st.inits[b.init as usize].in_d)
        .map(|b| (b.addr, b.addr + b.size as usize))
        .collect();
    cand.sort_unstable();
    st.cand_writes = vec![0; cand.len()];
    st.cand_ptr_writes = vec![0; cand.len()];
    st.cand = cand;
    crate::arena::arena_start_fresh_general_block();
    let mut last = 0usize;
    for i in 0..st.cand.len() {
        let (a, e) = st.cand[i];
        let mut p = a & !(PAGE - 1);
        while p < e {
            if p != last {
                libc::mprotect(p as *mut _, PAGE, libc::PROT_READ);
                last = p;
            }
            p += PAGE;
        }
    }
    st.rset_active = true;
    libc::atexit(rset_report);
}

fn cand_index(st: &State, word: usize) -> Option<usize> {
    let i = st.cand.partition_point(|&(a, _)| a <= word);
    (i > 0 && word < st.cand[i - 1].1).then(|| i - 1)
}

fn heap_pointer(st: &State, v: u64) -> bool {
    let hi = v >> 48;
    if hi != 0 && hi < 0x7FF9 {
        return false;
    }
    let a = (v & 0xFFFF_FFFF_FFFF) as usize;
    st.heap.slot(a).is_some()
}

extern "C" fn rset_report() {
    let _input_logging = input_record::Scope::enter();
    let st = state();
    if !st.rset_active {
        return;
    }
    st.rset_active = false;
    use std::io::Write;
    let Ok(mut f) = std::fs::File::create(format!("{}.rset.tsv", st.out)) else {
        return;
    };
    let cells = st.cand_writes.iter().filter(|&&n| n > 0).count();
    let ptr_cells = st.cand_ptr_writes.iter().filter(|&&n| n > 0).count();
    let writes: u64 = st.cand_writes.iter().map(|&n| n as u64).sum();
    let ptr_writes: u64 = st.cand_ptr_writes.iter().map(|&n| n as u64).sum();
    let _ = writeln!(
        f,
        "#rset\tcandidates={}\tcells_written={cells}\twrites={writes}\tptr_cells={ptr_cells}\tptr_writes={ptr_writes}\tfaults={}",
        st.cand.len(),
        st.rset_faults
    );
    // Writes per cell (histogram) and the most written cells, by birth.
    let mut hist = [0u64; 24];
    for &n in &st.cand_writes {
        if n > 0 {
            hist[(32 - n.leading_zeros()) as usize] += 1;
        }
    }
    for (b, n) in hist.iter().enumerate() {
        if *n > 0 {
            let _ = writeln!(f, "#hist\t<{}\t{n}", 1u64 << b);
        }
    }
    let mut top: Vec<usize> = (0..st.cand.len())
        .filter(|&i| st.cand_writes[i] > 0)
        .collect();
    top.sort_unstable_by_key(|&i| std::cmp::Reverse(st.cand_writes[i]));
    let births = st.births.as_slice();
    for &i in top.iter().take(200) {
        let (a, e) = st.cand[i];
        let init = births.iter().find(|b| b.addr == a).map_or(0, |b| b.init);
        let ty = unsafe { (*(a as *const crate::gc::GcHeader)).obj_type };
        let _ = writeln!(
            f,
            "{a:x}\t{}\t{:x}\t{}\t{}\t{}\t{}",
            init,
            st.inits[init as usize].addr,
            crate::gc::gc_type_info(ty).map_or("?", |i| i.name),
            e - a,
            st.cand_writes[i],
            st.cand_ptr_writes[i]
        );
    }
}

/// The boundary collection's census visits a live object.
pub(super) fn census_live(header: *const u8, obj_type: u8, size: usize) {
    let p = ANCHOR.state.load(Relaxed);
    if p.is_null() {
        return;
    }
    let st = unsafe { &mut *p };
    if st.boundary_seq == 0 || st.boundary_done {
        return;
    }
    let a = header as usize;
    let b = st.births.as_slice();
    let i = b.partition_point(|x| x.addr <= a);
    if i == 0 || a >= b[i - 1].addr + b[i - 1].size as usize {
        st.live_unattributed[0] += 1;
        st.live_unattributed[1] += size as u64;
        return;
    }
    let init = b[i - 1].init;
    let row = st.live_by.entry((init, obj_type)).or_insert([0; 2]);
    row[0] += 1;
    row[1] += size as u64;
}

pub(super) fn after_boundary_collection() {
    let p = ANCHOR.state.load(Relaxed);
    if p.is_null() {
        return;
    }
    let st = unsafe { &mut *p };
    if st.boundary_seq == 0 || st.boundary_done {
        return;
    }
    st.boundary_done = true;
    use std::io::Write;
    let path = format!("{}.live.tsv", st.out);
    if let Ok(mut f) = std::fs::File::create(&path) {
        let _ = writeln!(
            f,
            "#unattributed\t{}\t{}",
            st.live_unattributed[0], st.live_unattributed[1]
        );
        let mut rows: Vec<_> = st.live_by.iter().collect();
        rows.sort();
        for ((id, t), row) in rows {
            let name = crate::gc::gc_type_info(*t).map_or("?", |i| i.name);
            let _ = writeln!(f, "{id}\t{name}\t{}\t{}", row[0], row[1]);
        }
    }
}

fn write_report(st: &mut State) {
    if st.host_rule || st.p_entry_rule {
        k2a_rows(st);
    }
    st.env_reads =
        input_record::env_rows(st).expect("cannot read durable environment observations");
    use std::io::Write;
    let Ok(mut f) = std::fs::File::create(&st.out) else {
        return;
    };
    let _ = writeln!(
        f,
        "#snap\tboundary={}\tseq={}\tentry_body_seq={}\tinits={}\tbirths={}\tw={}\tr={}\tfaults={}\tignored={}\thost_outside={}\twall_ms={}\tperf={}",
        st.boundary_at,
        st.boundary_seq,
        st.entry_body_seq,
        st.inits.len() - 1,
        st.births.len,
        st.w.len,
        st.r.len,
        st.faults_total,
        st.faults_ignored,
        st.host_outside,
        st.t0.elapsed().as_millis(),
        st.perf_fd >= 0,
    );
    let _ = writeln!(
        f,
        "#meta\trule={}\tinstalls={}\towned={}\tmarks={}",
        st.meta_rule as u8, st.meta_installs, st.meta_owned, st.marks
    );
    let _ = writeln!(
        f,
        "#gkey\trule={}\tglobal={:x}\tkeyed_reads={}\tkeyed_writes={}\tunkeyed_reads={}\tunkeyed_writes={}\tkeyed_conflicts={}",
        st.gkey_rule as u8,
        st.gshape,
        st.gkey_counts[0],
        st.gkey_counts[1],
        st.gkey_counts[2],
        st.gkey_counts[3],
        st.gkey_conflicts.len()
    );
    for (init, key, name, len) in st.gkey_conflicts.iter() {
        let _ = writeln!(
            f,
            "#gkeyconflict\t{init}\t{key:x}\t{}",
            tsv_text(&String::from_utf8_lossy(&name[..*len as usize]))
        );
    }
    let _ = writeln!(
        f,
        "#id\taddr\tparent\treach\tdynamic\tfrom_entry_body\tenter\texit\topen\tthrew\tseeded\tin_d\tir\tbirths\tbirth_bytes\tfaults\thost_count\thost_class\thost_what\thost_pcs\tconflict_rule\tconflict_word\tconflict_write\tconflict_pc\tconflict_owner\tconflict_cause\tconflict_cause_pc\tdyn_fns\tevals"
    );
    let births = {
        let mut v: Vec<(usize, usize, u32)> = st
            .births
            .as_slice()
            .iter()
            .map(|b| (b.addr, b.addr + b.size as usize, b.init))
            .collect();
        v.sort_by_key(|b| b.0);
        v
    };
    let owner = |word: usize| -> String {
        let i = births.partition_point(|&(a, _, _)| a <= word);
        if i > 0 && word < births[i - 1].1 {
            let (a, e, init) = births[i - 1];
            let ty = unsafe { (*(a as *const crate::gc::GcHeader)).obj_type };
            format!(
                "birth:{init}@{a:x}:{}:{}+{}",
                crate::gc::gc_type_info(ty).map_or("?", |i| i.name),
                e - a,
                word - a
            )
        } else {
            "static".into()
        }
    };
    for (id, i) in st.inits.iter().enumerate() {
        let (hc, hw, hp) = match i.host {
            Some(h) => (
                h.class.to_string(),
                h.what.to_string(),
                h.pcs
                    .iter()
                    .map(|p| format!("{p:x}"))
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            None => ("".into(), "".into(), "".into()),
        };
        let (cr, cw, cwr, cpc, co, cc, ccpc) = match i.conflict {
            Some(c) => (
                c.rule.to_string(),
                format!("{:x}", c.word),
                (c.write as u8).to_string(),
                format!("{:x}", c.pc),
                owner(c.word),
                c.cause.to_string(),
                format!("{:x}", c.cause_pc),
            ),
            None => Default::default(),
        };
        let _ = writeln!(
            f,
            "{id}\t{:x}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{hc}\t{hw}\t{hp}\t{cr}\t{cw}\t{cwr}\t{cpc}\t{co}\t{cc}\t{ccpc}\t{}\t{}",
            i.addr,
            i.parent,
            i.reach as u8,
            i.dynamic as u8,
            i.from_entry_body as u8,
            i.enter_seq,
            i.exit_seq,
            i.open as u8,
            i.threw as u8,
            i.seeded as u8,
            i.in_d as u8,
            i.ir,
            i.births,
            i.birth_bytes,
            i.faults,
            i.host_count,
            i.dyn_fns,
            i.evals,
        );
    }
    // Step 6: a digest of the imaged heap per init. A birth is named by its
    // init's evaluation address and its rank among that init's births; a
    // word that holds a pointer into the reservation hashes as the name of
    // the object it points into (plus the offset), a word a member of D
    // wrote is left out (the image holds it as it was before), any other
    // word hashes as itself.
    //
    // The digest reads each logged birth as the object it was born as and
    // follows its words (a program's word count, a meta record, a weak
    // storage's entries): the log is the image only while the census holds
    // collection, so nothing moved or died. The weights run
    // (`PERRY_SNAP_NOPROTECT=1`) collects as production does: its log keeps
    // births whose blocks were freed or handed out again, so it writes no
    // digest (its report is the Ir per init above).
    if !st.noprotect {
        let log = st.births.as_slice();
        let mut rank: std::collections::HashMap<u32, u64> = std::collections::HashMap::new();
        let mut names: Vec<(usize, usize, u64)> = Vec::with_capacity(log.len());
        let mut ranks: Vec<u64> = Vec::with_capacity(log.len());
        // The env object and its storage are the env block, filled at
        // startup (an `env_object` cell): one name, never hashed by value.
        let mut env_store = st.env_storage.clone();
        env_store.sort_unstable();
        let is_env = |a: usize| env_store.binary_search_by_key(&a, |r| r.0).is_ok();
        // R1: a rolled-back word reads as it was before D (first write).
        let rolled_map: std::collections::HashMap<u64, u64> = {
            let mut m = std::collections::HashMap::new();
            for &(w, v) in st.repw_old.as_slice() {
                m.entry(w).or_insert(v);
            }
            m
        };
        let rolled = |w: usize| -> u64 {
            rolled_map
                .get(&(w as u64))
                .copied()
                .unwrap_or_else(|| unsafe { *(w as *const u64) })
        };
        let fnv = |h: u64, x: u64| (h ^ x).wrapping_mul(0x100000001B3);
        // R2 (design §2.10.16): a shared immutable cell is a value. 1 a
        // string (its bytes), 2 a regex program (its program words), 3 a
        // canonical key list (its keys, named in a second pass), 4 a
        // program's automaton storage (derived state of its program).
        let vkind = |b: &Birth| -> u8 {
            if !st.r2 {
                return 0;
            }
            let g = unsafe { &*(b.addr as *const crate::gc::GcHeader) };
            match g.obj_type {
                crate::gc::GC_TYPE_STRING => 1,
                crate::gc::GC_TYPE_REGEX_PROGRAM => 2,
                crate::gc::GC_TYPE_ARRAY if g.gc_flags & crate::gc::GC_FLAG_SHAPE_SHARED != 0 => 3,
                crate::gc::GC_TYPE_REGEX_CACHE => 4,
                _ => 0,
            }
        };
        let mut vk: Vec<u8> = Vec::with_capacity(log.len());
        let mut keylists: Vec<usize> = Vec::new();
        for b in log {
            let k = rank.entry(b.init).or_insert(0);
            let a = st.inits[b.init as usize].addr as u64;
            let kind = if is_env(b.addr) { 0 } else { vkind(b) };
            vk.push(kind);
            let user = b.addr + crate::gc::GC_HEADER_SIZE;
            let end = b.addr + b.size as usize;
            // The runtime's births (init 0) happen when an init first uses a
            // runtime object, so their order depends on D: they are named by
            // type and size only.
            let name = if is_env(b.addr) {
                ENV_NAME
            } else if kind == 1 {
                let byte_len = (rolled(user) >> 32) as usize;
                let p =
                    crate::string::string_data(user as *const crate::string::StringHeader) as usize;
                let n = byte_len.min(end.saturating_sub(p));
                let bytes = unsafe { std::slice::from_raw_parts(p as *const u8, n) };
                bytes
                    .iter()
                    .fold(fnv(0x5354_5249_4e47_0000, n as u64), |h, &c| {
                        fnv(h, c as u64)
                    })
            } else if kind == 2 {
                let words = unsafe { crate::regex::perex_owner::snap_program_words(user) };
                words
                    .iter()
                    .fold(fnv(0x5052_4f47_0000_0000, words.len() as u64), |h, &x| {
                        fnv(h, x as u64)
                    })
            } else if kind == 3 {
                keylists.push(b.addr);
                0
            } else if kind == 4 {
                // A regex program's lazy automaton storage (`GC_TYPE_REGEX_CACHE`)
                // is derived state of its program (owner ruling 2026-11-03): it
                // holds no program value, only states the matcher built from the
                // program, so it is one fixed name with no rank and no words. If
                // it ever holds a program value the digest hashes it.
                0x5245_4341_4348_4500
            } else if b.init == 0 {
                let ty = unsafe { (*(b.addr as *const crate::gc::GcHeader)).obj_type } as u64;
                0x5254_0000_0000_0000 ^ (ty << 32) ^ b.size as u64
            } else {
                a.wrapping_mul(0x100000001B3) ^ *k
            };
            names.push((b.addr, b.addr + b.size as usize, name));
            ranks.push(*k);
            if kind == 0 {
                *k += 1;
            }
        }
        let birth_names: Vec<u64> = names.iter().map(|n| n.2).collect();
        let mut wd_out = if st.skip.is_empty() {
            std::fs::File::create(format!("{}.wd", st.out)).ok()
        } else {
            None
        };
        names.sort_unstable();
        // A pointer word as a name: the cell it points into plus offset and tag.
        fn pname(names: &[(usize, usize, u64)], w: u64) -> u64 {
            let v = (w & 0xFFFF_FFFF_FFFF) as usize;
            let i = names.partition_point(|&(a, _, _)| a <= v);
            if i > 0 && v < names[i - 1].1 {
                let (n, o) = (names[i - 1].2, v - names[i - 1].0);
                n ^ (o as u64).rotate_left(17) ^ (w >> 48).rotate_left(40)
            } else {
                w
            }
        }
        // The words of a canonical key list's first `n` keys (after the
        // key-attribute reserve), as the image holds them.
        let key_words = |user: usize, end: usize, n: usize| -> Vec<u64> {
            let attrs = unsafe { (*(user as *const crate::gc::GcHeader).sub(1))._reserved }
                & crate::gc::GC_ARRAY_KEY_ATTRS
                != 0;
            let first =
                user + std::mem::size_of::<crate::array::ArrayHeader>() + if attrs { 8 } else { 0 };
            (0..n)
                .map(|i| first + 8 * i)
                .take_while(|&w| w + 8 <= end)
                .map(|w| rolled(w))
                .collect()
        };
        // Key-name rule (design §2.10.18): a key is named by what it is, never
        // by where it lives. A key in a named cell takes that cell's name: a
        // string by its bytes (R2), a fresh `Symbol()` by its birth (it is an
        // arena cell, `symbol::alloc_symbol`, born by the init that made it;
        // no later code obtains it except through a censused read, so a D
        // symbol held by an imaged init is visible as rule 2). A persistent
        // symbol (`symbol::leak_symbol`: no birth) is a cell of the process
        // immortal symbol space, admitted by the space's own range and cell
        // stride (`symbol::in_symbol_space`, design 2.10.19). Its cells are
        // bump allocated in creation order, so its address (and its id) is a
        // creation ordinal: an init in D that creates a persistent symbol
        // first shifts every later one. It is a registry (`Symbol.for`),
        // well-known, Intl or runtime symbol: any later code obtains the same
        // symbol by its name, whichever init created it, so it is named by
        // its kind bits (`Symbol.for`, well-known) and its description
        // bytes, not by its creator nor by its address. Any other foreign
        // key word hashes as its tag, like every foreign word of the digest.
        // `PERRY_SNAP_KEYNAME=0`: raw words.
        let kname = |names: &[(usize, usize, u64)], w: u64| -> u64 {
            if st.keyname && st.r2 {
                // A shape's string key is its bytes. Module pools may register
                // its image atom only when they run, so omitting D can leave an
                // equal heap atom (or inline string) in the canonical list.
                let mut short = [0; crate::value::SHORT_STRING_MAX_LEN];
                if let Some(bytes) = unsafe {
                    crate::string::js_string_key_bytes(crate::JSValue::from_bits(w), &mut short)
                } {
                    let name = bytes
                        .iter()
                        .fold(fnv(0x5354_5249_4e47_0000, bytes.len() as u64), |h, &c| {
                            fnv(h, c as u64)
                        });
                    // The ordinary R2 string cell's user-pointer name.
                    return name
                        ^ (crate::gc::GC_HEADER_SIZE as u64).rotate_left(17)
                        ^ 0x7fff_u64.rotate_left(40);
                }
            }
            let a = (w & 0xFFFF_FFFF_FFFF) as usize;
            let named = pname(names, w);
            if !st.keyname || named != w || !crate::symbol::in_symbol_space(a) {
                return named;
            }
            let g = unsafe { &*((a - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader) };
            if g.obj_type != crate::gc::GC_TYPE_SYMBOL {
                return fnv(0x464f_5245_4947_4e00, w >> 48);
            }
            let sym = a as *const crate::symbol::SymbolHeader;
            // The description by its bytes (a string is a value, R2), wherever it lives: a
            // leaked symbol keeps it as native text and its header's `description` is null,
            // so it is read through `symbol_description_text` (the one reader of both homes).
            let dn = match unsafe { crate::symbol::symbol_description_text(sym) } {
                None => 0,
                Some(b) => b
                    .iter()
                    .fold(fnv(0x5354_5249_4e47_0000, b.len() as u64), |h, &c| {
                        fnv(h, c as u64)
                    }),
            };
            fnv(
                fnv(
                    fnv(0x5359_4d42_0000_0000, w >> 48),
                    (unsafe { (*sym).registered }
                        & (perry_abi::SYMBOL_GLOBAL_REGISTRY | perry_abi::SYMBOL_WELL_KNOWN))
                        as u64,
                ),
                dn,
            )
        };
        if !keylists.is_empty() {
            keylists.sort_unstable();
            let upd: Vec<(usize, u64)> = names
                .iter()
                .enumerate()
                .filter(|(_, n)| keylists.binary_search(&n.0).is_ok())
                .map(|(i, n)| {
                    let user = n.0 + crate::gc::GC_HEADER_SIZE;
                    let len = (rolled(user) & 0xFFFF_FFFF) as usize;
                    if st.backing {
                        let root = key_words(user, n.1, len.min(1))
                            .into_iter()
                            .fold(fnv(0x4b45_5953_524f_4f54, (len != 0) as u64), |h, w| {
                                fnv(h, kname(&names, w))
                            });
                        return (i, root);
                    }
                    let h = key_words(user, n.1, len)
                        .into_iter()
                        .fold(fnv(0x4b45_5953_0000_0000, len as u64), |h, w| {
                            fnv(h, kname(&names, w))
                        });
                    (i, h)
                })
                .collect();
            for (i, h) in upd {
                names[i].2 = h;
            }
        }
        // A shape names its key list by its first `count` keys (R2): a list
        // extended in place later is the same list to this shape.
        let shape_keys = |keys: u64, count: usize| -> u64 {
            let v = (keys & 0xFFFF_FFFF_FFFF) as usize;
            let i = names.partition_point(|&(a, _, _)| a <= v);
            if !st.r2
                || i == 0
                || v >= names[i - 1].1
                || v != names[i - 1].0 + crate::gc::GC_HEADER_SIZE
            {
                return pname(&names, keys);
            }
            key_words(v, names[i - 1].1, count)
                .into_iter()
                .fold(fnv(0x4b45_5953_0000_0000, count as u64), |h, w| {
                    fnv(h, kname(&names, w))
                })
        };
        // Diagnostic (the dig dump): a shape key input key by key: L (a named heap
        // list) or P (pointer named by pname), then each key word's raw value and name.
        let shape_keys_tok = |keys: u64, count: usize| -> String {
            let v = (keys & 0xFFFF_FFFF_FFFF) as usize;
            let i = names.partition_point(|&(a, _, _)| a <= v);
            if !st.r2
                || i == 0
                || v >= names[i - 1].1
                || v != names[i - 1].0 + crate::gc::GC_HEADER_SIZE
            {
                let (n, o) = if i > 0 && v < names[i - 1].1 {
                    (names[i - 1].2, v - names[i - 1].0)
                } else {
                    (0, 0)
                };
                return format!("P{keys:x}={n:x}+{o}");
            }
            let ws = key_words(v, names[i - 1].1, count);
            let mut t = format!("L{}", ws.len());
            for w in ws {
                t.push_str(&format!(",{w:x}={:x}", kname(&names, w)));
            }
            t
        };
        // Serial rule (design §2.10.17): a dynamic prototype serial
        // (`FIRST_DYNAMIC_PROTOTYPE_SERIAL` and up, drawn in allocation
        // order, so an init's serial depends on how many D drew before it)
        // is named by the birth name of the object whose meta record carries
        // it, the way a ShapeId is named by its shape. Intrinsic serials
        // (below it) and serials of no birth (image prototypes, families
        // registered before user code) are fixed numbers and hash as such.
        let serial_names: std::collections::HashMap<u64, u64> = if st.serial {
            names
                .iter()
                .filter_map(|&(a, _, n)| {
                    let bits =
                        crate::value::js_nanbox_pointer((a + crate::gc::GC_HEADER_SIZE) as i64)
                            .to_bits();
                    let s = unsafe { crate::object::shapes::prototype_serial(bits) };
                    (s >= crate::object::proto_validity::FIRST_DYNAMIC_PROTOTYPE_SERIAL
                        && s < crate::object::shapes::PROTO_ID_CLASS)
                        .then_some((s, n))
                })
                .collect()
        } else {
            Default::default()
        };
        // A shape's prototype identity with its serial named: a plain serial
        // identity, or a MIXED one (class id beside the serial). None: not a
        // named dynamic serial (the identity hashes as before).
        let pser = |id: u64| -> Option<u64> {
            use crate::object::shapes::{
                PROTO_ID_CLASS, PROTO_ID_MIXED, PROTO_ID_MIXED_SERIAL_BITS, PROTO_ID_UNIQUE,
            };
            let (hi, s) = if id < PROTO_ID_CLASS {
                (0, id)
            } else if (PROTO_ID_MIXED..PROTO_ID_UNIQUE).contains(&id) {
                let m = (1u64 << PROTO_ID_MIXED_SERIAL_BITS) - 1;
                (id & !m, id & m)
            } else {
                return None;
            };
            if s < crate::object::proto_validity::FIRST_DYNAMIC_PROTOTYPE_SERIAL {
                return None;
            }
            serial_names
                .get(&s)
                .map(|&n| fnv(fnv(0x5345_5249_414c_0000, hi), n))
        };
        let name_of = |v: usize| -> Option<(u64, usize)> {
            let i = names.partition_point(|&(a, _, _)| a <= v);
            (i > 0 && v < names[i - 1].1).then(|| (names[i - 1].2, v - names[i - 1].0))
        };
        let mut per: std::collections::BTreeMap<usize, (u64, u64, u64)> = Default::default();
        // Diagnostic: `PERRY_SNAP_DIGEST_DUMP=1` writes every hashed word
        // (`<out>.dig`: init, birth name, offset, token) to diff two runs.
        let mut dig_out = std::env::var_os("PERRY_SNAP_DIGEST_DUMP")
            .and_then(|_| std::fs::File::create(format!("{}.dig", st.out)).ok());
        let mut dump = |init: usize, bname: u64, off: usize, tok: String| {
            if let Some(o) = dig_out.as_mut() {
                use std::io::Write;
                let _ = writeln!(o, "{init:x}\t{bname:x}\t{off}\t{tok}");
            }
        };
        let mix = |h: &mut u64, x: u64| {
            *h ^= x;
            *h = h.wrapping_mul(0x100000001B3);
        };
        // A pointer outside the reservation and the binary points into
        // malloc memory (mimalloc picks randomized arena hints, so its
        // addresses differ per run even with ASLR off): it hashes as its tag.
        // A raw (untagged) word that points into mimalloc's arenas is such
        // a pointer too: runtime objects keep native pointers (a closure's
        // Rust-side state) in raw words.
        let foreign = |v: u64| -> bool {
            let a = (v & 0xFFFF_FFFF_FFFF) as usize;
            let hi = v >> 48;
            if hi == 0 {
                return in_malloc(a);
            }
            (hi == 0x7FFD || hi == 0x7FFC || hi == 0x7FFF || hi == 0x7FFE)
                && st.heap.slot(a).is_none()
                && st.bin.slot(a).is_none()
                && tls_slot(st, a).is_none()
        };
        for (bk, b) in log.iter().enumerate() {
            if (b.init != 0 && st.inits[b.init as usize].in_d) || is_env(b.addr) {
                continue;
            }
            // R2: a value is no init's state; pointers name it by content.
            if vk[bk] != 0 {
                continue;
            }
            let bname = birth_names[bk];
            let e = per
                .entry(st.inits[b.init as usize].addr)
                .or_insert((0xcbf29ce484222325, 0, 0));
            e.1 += 1;
            let ty = unsafe { (*(b.addr as *const crate::gc::GcHeader)).obj_type };
            // Slack rule (§2.10.20): an ordinary object allocated exactly at
            // its shape's live bound is named up to its key count; the slots
            // past it are capacity (the image keeps its own run's width: the
            // shape and the allocation agree, a later key spills as today).
            // An Ordinary object allocated past its live bound (a floor birth
            // whose zero-bound shape hides the floor slots, while a tracking
            // birth in the other run is born at its learned width) is named
            // the same way: the shape never addresses capacity past `live`.
            // An arguments object keeps a private tail at the allocation end,
            // so it is never widened.
            let slack_end = if st.slack && ty == crate::gc::GC_TYPE_OBJECT {
                let base =
                    crate::gc::GC_HEADER_SIZE + std::mem::size_of::<crate::object::ObjectHeader>();
                let w = unsafe { *((b.addr + crate::gc::GC_HEADER_SIZE) as *const u64) };
                crate::object::shapes::shape_descriptor_for_word(crate::codegen_abi::LayoutWord(
                    (w >> 32) as u32,
                ))
                .and_then(|d| {
                    let live = d.live_inline_slot_count as usize;
                    let size = b.size as usize;
                    let widened = size > base + 8 * live
                        && d.object_kind == crate::object::shapes::ShapeObjectKind::Ordinary
                        && unsafe { (*(b.addr as *const crate::gc::GcHeader))._reserved }
                            & crate::gc::OBJ_FLAG_ARGUMENTS
                            == 0;
                    (size == base + 8 * live || widened)
                        .then(|| base + 8 * live.min(d.logical_key_count as usize))
                })
            } else {
                None
            };
            dump(
                st.inits[b.init as usize].addr,
                bname,
                0,
                format!("t{ty}:{}", slack_end.unwrap_or(b.size as usize)),
            );
            // R3 (design §2.10.16): a WeakStorage's bucket positions and
            // entry order are derived state (they hash raw addresses): it
            // digests its live entry set, sorted.
            if st.r3 && ty == crate::gc::GC_TYPE_WEAK_STORAGE {
                let ws = (b.addr + crate::gc::GC_HEADER_SIZE)
                    as *const crate::weakref::storage::WeakStorage;
                let mut ents: Vec<(u64, u64)> = Vec::new();
                unsafe {
                    let n = (*ws).len.min((*ws).capacity) as usize;
                    let base = (*ws).entries();
                    for i in 0..n {
                        let ent = base.add(i);
                        if (*ent).key == crate::value::TAG_UNDEFINED {
                            continue;
                        }
                        let kw = std::ptr::addr_of!((*ent).key) as usize;
                        let mut skip = false;
                        for w in [kw, kw + 8] {
                            let o = w - b.addr;
                            if st.w.contains(w as u64) || st.repw.contains(w as u64) {
                                if let Some(f) = wd_out.as_mut() {
                                    use std::io::Write;
                                    let _ = writeln!(f, "{bname:x}\t{o}");
                                }
                                skip = true;
                            } else if st.wd_names.contains(&(bname, o)) {
                                skip = true;
                            }
                        }
                        if !skip {
                            ents.push((pname(&names, (*ent).key), pname(&names, (*ent).value)));
                        }
                    }
                }
                ents.sort_unstable();
                let h = ents
                    .iter()
                    .fold(0xcbf29ce484222325u64, |h, &(k, v)| fnv(fnv(h, k), v));
                e.2 += 1;
                mix(&mut e.0, h);
                dump(
                    st.inits[b.init as usize].addr,
                    bname,
                    crate::gc::GC_HEADER_SIZE,
                    format!("k{h:x}:{}", ents.len()),
                );
                continue;
            }
            // The GC header (mark and move-request bits) is no JS state.
            let mut off = crate::gc::GC_HEADER_SIZE;
            // A Map store's native indexes are derived state (see
            // `map::snap_native_index_range`): left out.
            let skip = if ty == crate::gc::GC_TYPE_MAP_STORE {
                let (s, e) = crate::map::snap_native_index_range();
                (crate::gc::GC_HEADER_SIZE + s, crate::gc::GC_HEADER_SIZE + e)
            } else {
                (0, 0)
            };
            while off + 8 <= b.size as usize {
                let word = b.addr + off;
                off += 8;
                if word - b.addr >= skip.0 && word - b.addr < skip.1 {
                    continue;
                }
                if slack_end.is_some_and(|e| word - b.addr >= e) {
                    continue;
                }
                if st.w.contains(word as u64) || st.repw.contains(word as u64) {
                    if let Some(o) = wd_out.as_mut() {
                        use std::io::Write;
                        let _ = writeln!(o, "{bname:x}\t{}", off - 8);
                    }
                    continue;
                }
                if st.wd_names.contains(&(bname, off - 8)) {
                    continue;
                }
                let v = unsafe { *(word as *const u64) };
                e.2 += 1;
                // The carriers of numbers drawn in creation order name them as
                // their uses do: a meta record's dynamic prototype serial by the
                // birth name of the object it belongs to (serial rule, §2.10.17;
                // `pser`, as a shape's prototype input), and a fresh symbol's id
                // by the symbol's own birth name (symbol identity, §2.10.18). A
                // serial of no birth hashes as its number, as before.
                let q = if st.serial
                    && ty == crate::gc::GC_TYPE_OBJECT_META
                    && off - 8
                        == crate::gc::GC_HEADER_SIZE
                            + std::mem::offset_of!(crate::object::ObjectMeta, proto_serial)
                {
                    pser(v)
                } else if st.symid
                    && ty == crate::gc::GC_TYPE_SYMBOL
                    && off - 8
                        == crate::gc::GC_HEADER_SIZE
                            + std::mem::offset_of!(crate::symbol::SymbolHeader, id)
                {
                    Some(fnv(0x5359_4d49_4400_0000, bname))
                } else {
                    None
                };
                if let Some(n) = q {
                    mix(&mut e.0, n);
                    dump(
                        st.inits[b.init as usize].addr,
                        bname,
                        off - 8,
                        format!("q{n:x}"),
                    );
                    continue;
                }
                // An ordinary object's first header word holds its class id
                // and its ShapeId (a closure's: capture count and ShapeId). A ShapeId is the shape's NAME: ids are
                // handed out in creation order, and the env object's keys
                // (filled at startup) take ids too, so the same shape gets
                // another id when the env block differs. It hashes as the
                // shape it names: key list, counts, flags, prototype link,
                // kind and representation.
                // An arguments object's tail word (`ArgumentsTail`: the length
                // in the low half, the birth ShapeId in the high half) is the
                // same kind of carrier: it hashes as the length and the shape.
                let tail_word = st.tail_shape
                    && ty == perry_abi::GC_TYPE_OBJECT
                    && off - 8 + 16 == b.size as usize
                    && unsafe { (*(b.addr as *const crate::gc::GcHeader))._reserved }
                        & crate::gc::OBJ_FLAG_ARGUMENTS
                        != 0;
                if tail_word
                    || off - 8 == crate::gc::GC_HEADER_SIZE
                        && (ty == perry_abi::GC_TYPE_OBJECT || ty == perry_abi::GC_TYPE_CLOSURE)
                {
                    let sid = (v >> 32) as u32;
                    if let Some(d) = crate::object::shapes::shape_descriptor_for_word(
                        crate::codegen_abi::LayoutWord(sid),
                    ) {
                        let pn = |w: u64| {
                            name_of((w & 0xFFFF_FFFF_FFFF) as usize).map_or(w, |(n, o)| {
                                n ^ (o as u64).rotate_left(17) ^ (w >> 48).rotate_left(40)
                            })
                        };
                        let parts = [
                            shape_keys(d.keys, d.logical_key_count as usize),
                            d.logical_key_count as u64,
                            if st.slack && ty == perry_abi::GC_TYPE_OBJECT {
                                d.live_inline_slot_count.min(d.logical_key_count) as u64
                            } else {
                                d.live_inline_slot_count as u64
                            },
                            d.integrity_flags as u64,
                            pser(d.proto_id).unwrap_or_else(|| pn(d.proto_id)),
                            d.object_kind as u8 as u64,
                            d.hole_count as u64,
                            d.summary as u64,
                            // A deprecated F64 lane (`10`) is learned record state, like the
                            // width above: field_rep::identity reads it as `F64`, and stores
                            // by D deprecate it in the census only (design §2.10.20).
                            if st.slack {
                                crate::object::field_rep::identity_with_special(d.rep)
                            } else {
                                d.rep
                            },
                            d.special_constfn_mask as u64,
                        ];
                        let sname = parts.iter().fold(0xcbf29ce484222325u64, |h, &x| {
                            (h ^ x).wrapping_mul(0x100000001B3)
                        });
                        mix(&mut e.0, v & 0xFFFF_FFFF);
                        mix(&mut e.0, sname);
                        // The dump shows the inputs too (keys, prototype, the rest), so a
                        // differing shape word names its differing field.
                        let rest = parts[5..]
                            .iter()
                            .fold(parts[1] ^ parts[2] << 20 ^ parts[3] << 40, |h, &x| {
                                (h ^ x).wrapping_mul(0x100000001B3)
                            });
                        let tok = format!(
                            "s{:x}:{sname:x}|k{:x}|p{:x}|r{rest:x}|K{}",
                            v & 0xFFFF_FFFF,
                            parts[0],
                            parts[4],
                            shape_keys_tok(d.keys, d.logical_key_count as usize)
                        );
                        dump(st.inits[b.init as usize].addr, bname, off - 8, tok);
                        continue;
                    }
                }
                let hi = v >> 48;
                if hi == 0 || hi >= 0x7FF9 {
                    if let Some((n, o)) = name_of((v & 0xFFFF_FFFF_FFFF) as usize) {
                        mix(&mut e.0, hi ^ 0xA5A5);
                        mix(&mut e.0, n);
                        mix(&mut e.0, o as u64);
                        dump(
                            st.inits[b.init as usize].addr,
                            bname,
                            off - 8,
                            format!("p{hi:x}:{n:x}+{o}"),
                        );
                        continue;
                    }
                }
                if foreign(v) {
                    mix(&mut e.0, v >> 48);
                    dump(
                        st.inits[b.init as usize].addr,
                        bname,
                        off - 8,
                        format!("f{:x}", v >> 48),
                    );
                    continue;
                }
                mix(&mut e.0, v);
                dump(
                    st.inits[b.init as usize].addr,
                    bname,
                    off - 8,
                    format!("v{v:x}"),
                );
            }
        }
        // The module-global slots each imaged init registered.
        for (k, &slot) in st.globals_log.iter().enumerate() {
            let owner = st.slot_owner.get(k).copied().unwrap_or(0);
            if owner == 0 || st.inits[owner as usize].in_d {
                continue;
            }
            // A slot a member of D wrote (the CJS support module's
            // `pendingParent`, set by a deferred require inside a D body) is
            // named in `.wd` like a birth word D wrote: the check, where D
            // never runs, leaves it out too.
            if st.w.contains(slot as u64) {
                if st.wd_slot {
                    if let Some(o) = wd_out.as_mut() {
                        use std::io::Write;
                        let _ = writeln!(o, "0\t{slot}");
                    }
                }
                continue;
            }
            if st.wd_slot && st.wd_names.contains(&(0, slot)) {
                continue;
            }
            let e = per
                .entry(st.inits[owner as usize].addr)
                .or_insert((0xcbf29ce484222325, 0, 0));
            let v = unsafe { *(slot as *const u64) };
            e.2 += 1;
            let hi = v >> 48;
            if hi == 0 || hi >= 0x7FF9 {
                if let Some((n, o)) = name_of((v & 0xFFFF_FFFF_FFFF) as usize) {
                    mix(&mut e.0, hi ^ 0xA5A5);
                    mix(&mut e.0, n);
                    mix(&mut e.0, o as u64);
                    dump(
                        st.inits[owner as usize].addr,
                        0,
                        slot,
                        format!("p{hi:x}:{n:x}+{o}"),
                    );
                    continue;
                }
            }
            if foreign(v) {
                mix(&mut e.0, v >> 48);
                dump(
                    st.inits[owner as usize].addr,
                    0,
                    slot,
                    format!("f{:x}", v >> 48),
                );
                continue;
            }
            mix(&mut e.0, v);
            dump(st.inits[owner as usize].addr, 0, slot, format!("v{v:x}"));
        }
        let mut total = 0xcbf29ce484222325u64;
        for (addr, (h, objs, words)) in &per {
            mix(&mut total, *h);
            let _ = writeln!(f, "#digest\t{addr:x}\t{h:016x}\t{objs}\t{words}");
        }
        let _ = writeln!(f, "#digest_total\t{total:016x}\t{}", per.len());
        write_env_report(st, &mut f, &ranks);
    }
    let _ = writeln!(f, "#excluded\t{}", st.excluded);
    let _ = writeln!(
        f,
        "#repw\trule={}\twords={}\twrites={}\towned={}\treads={}\tr2={}\tr3={}\tserial={}\tkeyname={}\tsymid={}",
        st.r1 as u8, st.repw.len, st.repw_writes, st.repw_owned, st.repw_reads, st.r2 as u8, st.r3 as u8,
        st.serial as u8, st.keyname as u8, st.symid as u8
    );
    let _ = writeln!(f, "#free_space\t{}", st.free_space);
    let _ = writeln!(f, "#reborn\t{}", st.reborn);
    let _ = writeln!(f, "#silent_stores\t{}", st.silent_stores);
    let _ = writeln!(
        f,
        "#transparent\t{}\t{}\t{}",
        st.transparent.len(),
        st.transp.len,
        st.transparent_hits
    );
    let _ = writeln!(f, "#foreign_thread_faults\t{}", st.foreign_faults);
    let _ = writeln!(
        f,
        "#untracked_slots\t{}\tof\t{}\tfirst={:x}",
        st.untracked_slots, st.globals.len, st.untracked_first
    );
    let _ = writeln!(
        f,
        "#runtime_tls_roots\t{}\tfirst={:x}\tblock={:x}-{:x}",
        st.runtime_tls_roots, st.runtime_tls_first, st.tls_block.0, st.tls_block.1
    );
    let r = st.tls_ranges;
    let _ = writeln!(
        f,
        "#tls\twhole={}\tdata={:x}-{:x}\tbss={:x}-{:x}",
        st.tls_whole as u8, r[0], r[1], r[2], r[3]
    );
    for (what, pcs) in &st.realm_hosts {
        let pcs: Vec<String> = pcs.iter().map(|p| format!("{p:x}")).collect();
        let _ = writeln!(f, "#realm_host\t{what}\t{}", pcs.join(","));
    }
    for (init, name, _) in &st.env_reads {
        let _ = writeln!(f, "#getenv\t{init}\t{name}");
    }
    // The next pass's seed.
    if let Ok(mut d) = std::fs::File::create(format!("{}.d", st.out)) {
        for i in st.inits.iter().skip(1) {
            if i.in_d {
                let _ = writeln!(d, "{:x}", i.addr);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// SNAP3: env cells, the image guard (design §2.10.3-§2.10.4)
// ---------------------------------------------------------------------------

/// What an env observation was (`EnvObs::kind`).
const OBS_OBJECT: u8 = 0;
/// A store-site read of a set var: a cell while no imaged init reads the
/// string's body.
const OBS_STORE: u8 = 1;
const OBS_OBSERVE: u8 = 2;
const OBS_HAS: u8 = 3;
/// A store-site read of an unset var (no pointer to find: a guard member).
const OBS_STORE_UNSET: u8 = 4;
/// A store-site string whose body an imaged init read.
const OBS_STORE_READ: u8 = 5;

/// `env_words` value of an env block word (the env object's storage).
const ENV_BLOCK: u32 = u32::MAX;
/// `env_words` value of a module-global slot a member of D registered in the
/// census pass (the check run poisons it).
const POISON: u32 = u32::MAX - 1;
/// At most this many poisoned reads are recorded (`check_fail`).
const CHECK_FAIL_MAX: usize = 4096;

/// `#checkfail\tpoison\t<init>\t<init addr>\t<slot>\t<pc>\t<write>` to
/// `<out>.checkfail` (signal context: no allocation). The report at the
/// boundary repeats the line with the slot's logical name.
fn checkfail_line(st: &State, init: u32, word: usize, pc: usize, write: bool) {
    if st.checkfail_fd < 0 {
        return;
    }
    let mut buf = [0u8; 160];
    let mut n = 0;
    let mut put = |b: &[u8], n: &mut usize| {
        for &c in b {
            buf[*n] = c;
            *n += 1;
        }
    };
    put(b"#checkfail\tpoison\t", &mut n);
    let mut dec = [0u8; 10];
    let mut k = dec.len();
    let mut x = init;
    loop {
        k -= 1;
        dec[k] = b'0' + (x % 10) as u8;
        x /= 10;
        if x == 0 {
            break;
        }
    }
    put(&dec[k..], &mut n);
    let addr = st.inits.get(init as usize).map_or(0, |i| i.addr);
    for w in [addr, word, pc] {
        put(b"\t", &mut n);
        let mut hex = [0u8; 16];
        let mut k = hex.len();
        let mut x = w;
        loop {
            k -= 1;
            hex[k] = b"0123456789abcdef"[x & 15];
            x >>= 4;
            if x == 0 {
                break;
            }
        }
        put(&hex[k..], &mut n);
    }
    put(if write { b"\t1\n" } else { b"\t0\n" }, &mut n);
    unsafe { libc::write(st.checkfail_fd, buf.as_ptr().cast(), n) };
}

/// The check run ends before the boundary (an exception, `process.exit`):
/// write the report there, so the run always ends with one.
extern "C" fn check_exit() {
    if state().boundary_seq == 0 {
        boundary("exit");
    }
}

struct EnvObs {
    init: u32,
    kind: u8,
    name: String,
    value: Option<String>,
    /// The string a store-site read returned (header address).
    ptr: usize,
    /// The imaged init that read a store-site string's body, and where.
    by: u32,
    by_pc: usize,
}

/// The entry's state (DESIGN27 §2): the entry pseudo-init or the entry
/// module's own evaluation. It re-runs live; nothing it holds is imaged.
fn is_entry(st: &State, id: u32) -> bool {
    id != 0 && (id == st.entry_init || st.inits[id as usize].entry_module)
}

fn host_name(what: u8) -> &'static str {
    match what {
        1 => "argv",
        2 => "exec_path",
        _ => "entry_birth",
    }
}

/// A host value is materialized next (`HostValueScope`): one `host_value`
/// row; the births until `host_value_exit` are its targets.
pub(super) fn host_value_enter(what: u8) -> bool {
    if !(inputs_on() && on_main_thread()) {
        return false;
    }
    let _input_logging = input_record::Scope::enter();
    let st = state();
    if !st.host_rule || st.host_what != 0 {
        return false;
    }
    let top = top_init(st);
    let by = if top == 0 {
        "runtime"
    } else if is_entry(st, top) {
        "entry"
    } else {
        "imaged"
    };
    let policy = if by == "entry" { "live" } else { "STOP" };
    input_record::event(
        st,
        "host_value",
        format!("what={};by={by};policy={policy}", host_name(what)),
    );
    st.host_what = what;
    true
}

pub(super) fn host_value_exit() {
    state().host_what = 0;
}

/// SNAPK2a scan at the boundary (DESIGN27 §1.3 and P-entry (c)): every word
/// outside entry state (births that are neither the entry's nor a host
/// value's, module-global slots no entry owns) that holds a pointer into a
/// host value or into an entry birth is a STOP. The store is no earlier than
/// both births, so the row is dated at the later one (`at_seq`). Then the
/// P-entry (a)/(b) hits logged during the run.
fn k2a_rows(st: &mut State) {
    let mut t: Vec<(usize, usize, u8, u64, u32)> = Vec::new();
    if st.host_rule {
        t.extend(st.host_targets.iter().map(|&(a, e, w, s)| (a, e, w, s, 0)));
    }
    if st.p_entry_rule {
        for b in st.births.as_slice() {
            if is_entry(st, b.init) {
                t.push((b.addr, b.addr + b.size as usize, 255, b.seq, b.init));
            }
        }
    }
    t.sort_unstable();
    // Owned storage (meta rule: a meta record's words are its owner's; its
    // spill, overflow, elements, expando and owned-state arrays are child
    // edges that move and die with the owner): an entry birth reached
    // through such an edge of a non-entry holder is that holder's
    // representation, grown by the entry's write to the holder (named by
    // key as P-entry (a)/(b)), not an entry value. It is no (c) target, and
    // its words are scanned as the holder's. Edges into adopted storage
    // (an entry-born meta's spill) are followed to a fixpoint.
    let owned: Vec<(usize, usize, u32)> = if st.p_entry_rule {
        let hs = crate::gc::GC_HEADER_SIZE;
        let meta_edges = [
            std::mem::offset_of!(crate::object::ObjectMeta, spill),
            std::mem::offset_of!(crate::object::ObjectMeta, overflow),
            std::mem::offset_of!(crate::object::ObjectMeta, elements),
            std::mem::offset_of!(crate::object::ObjectMeta, expando),
            std::mem::offset_of!(crate::object::ObjectMeta, owned_state),
        ];
        let obj_meta = std::mem::offset_of!(crate::object::ObjectHeader, meta);
        let entry_at = |v: u64| -> Option<usize> {
            let a = (v & 0xFFFF_FFFF_FFFF) as usize;
            let i = t.partition_point(|x| x.0 <= a);
            (a != 0 && i > 0 && t[i - 1].2 == 255 && (a == t[i - 1].0 || a == t[i - 1].0 + hs))
                .then(|| i - 1)
        };
        let mut adopted: Vec<Option<u32>> = vec![None; t.len()];
        let mut work: Vec<(usize, u32)> = st
            .births
            .as_slice()
            .iter()
            .filter(|b| !is_entry(st, b.init))
            .map(|b| (b.addr, b.init))
            .collect();
        while let Some((h, init)) = work.pop() {
            let ty = unsafe { (h as *const u8).read() };
            let edges: &[usize] = match ty {
                crate::gc::GC_TYPE_OBJECT_META => &meta_edges,
                crate::gc::GC_TYPE_OBJECT => std::slice::from_ref(&obj_meta),
                _ => &[],
            };
            for &e in edges {
                let v = unsafe { ((h + hs + e) as *const u64).read() };
                if let Some(k) = entry_at(v) {
                    if adopted[k].is_none() {
                        adopted[k] = Some(init);
                        work.push((t[k].0, init));
                    }
                }
            }
        }
        let mut keep = Vec::with_capacity(t.len());
        let mut owned = Vec::new();
        for (k, x) in t.iter().enumerate() {
            match adopted[k] {
                Some(init) => owned.push((x.0, x.1, init)),
                None => keep.push(*x),
            }
        }
        t = keep;
        owned
    } else {
        Vec::new()
    };
    let find = |v: u64| -> Option<usize> {
        let hi = v >> 48;
        if !(hi == 0 || (0x7FFC..=0x7FFF).contains(&hi)) {
            return None;
        }
        let a = (v & 0xFFFF_FFFF_FFFF) as usize;
        let i = t.partition_point(|x| x.0 <= a);
        (a != 0 && i > 0 && a < t[i - 1].1).then(|| i - 1)
    };
    let is_target = |a: usize| {
        let i = t.partition_point(|x| x.0 <= a);
        i > 0 && t[i - 1].0 == a
    };
    let mut rows: Vec<(&'static str, String)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut held = 0u64;
    let row = |rows: &mut Vec<(&'static str, String)>, k: usize, holder: String, at: u64| {
        let (op, rule) = if t[k].2 == 255 {
            ("p_entry", "rule=c;")
        } else {
            ("host_value.held", "")
        };
        rows.push((
            op,
            format!(
                "{rule}what={};{holder};target_init={};target_seq={};at_seq={at};policy=STOP",
                host_name(t[k].2),
                t[k].4,
                t[k].3
            ),
        ));
    };
    if !t.is_empty() {
        let holders = st
            .births
            .as_slice()
            .iter()
            .filter(|b| !is_entry(st, b.init))
            .map(|b| (b.addr, b.size as usize, b.init, b.seq))
            .chain(owned.iter().map(|&(a, e, init)| (a, e - a, init, 0)));
        for (addr, size, init, seq) in holders {
            if is_target(addr) {
                continue;
            }
            let b = Birth {
                addr,
                size: size as u32,
                init,
                seq,
            };
            for off in (8..b.size as usize).step_by(8) {
                let v = unsafe { ((b.addr + off) as *const u64).read() };
                if let Some(k) = find(v) {
                    held += 1;
                    if seen.insert((b.addr, k)) && rows.len() < CHECK_FAIL_MAX {
                        row(
                            &mut rows,
                            k,
                            format!(
                                "holder=birth;holder_init={};holder_type={};holder_seq={};holder_addr={:x};off={off}",
                                b.init,
                                unsafe { (b.addr as *const u8).read() },
                                b.seq,
                                b.addr
                            ),
                            b.seq.max(t[k].3),
                        );
                    }
                }
            }
        }
        for (n, &slot) in st.globals_log.iter().enumerate() {
            let owner = st.slot_owner.get(n).copied().unwrap_or(0);
            if is_entry(st, owner) {
                continue;
            }
            let v = unsafe { (slot as *const u64).read() };
            if let Some(k) = find(v) {
                held += 1;
                if seen.insert((slot, k)) && rows.len() < CHECK_FAIL_MAX {
                    row(
                        &mut rows,
                        k,
                        format!("holder=slot;holder_init={owner};slot={}", slot_name(slot)),
                        t[k].3,
                    );
                }
            }
        }
    }
    for &(init, rule, word, pc, seq) in &st.p_entry_log {
        rows.push((
            "p_entry",
            format!(
                "rule={};init={init};word={word:x};pc={pc:x};at_seq={seq};policy=STOP",
                rule as char
            ),
        ));
    }
    let summary = format!(
        "host_targets={};entry_births={};held_words={held};p_entry_ab={}",
        st.host_targets.len(),
        t.iter().filter(|x| x.2 == 255).count(),
        st.p_entry_log.len()
    );
    for (op, v) in rows {
        input_record::event(st, op, v);
    }
    input_record::event(st, "k2a", summary);
}

fn top_init(st: &State) -> u32 {
    st.frames.last().map_or(0, |f| f.init)
}

/// Watch `[lo, hi)` as env words with value `v`: their pages stay closed
/// whatever runs.
fn watch_env_words(st: &mut State, lo: usize, hi: usize, v: u32) {
    let mut w = lo & !7;
    while w < hi {
        st.env_words.insert(w as u64, v);
        w += 8;
    }
    let mut p = lo & !(PAGE - 1);
    while p < hi {
        flag_page(st, p, PF_ENV);
        if !st.noprotect {
            unsafe { libc::mprotect(p as *mut _, PAGE, libc::PROT_NONE) };
        }
        p += PAGE;
    }
}

pub(super) fn births_len() -> usize {
    state().births.len
}

pub(super) fn env_site(kind: i32) {
    let _input_logging = input_record::Scope::enter();
    let st = state();
    st.env_site = Some((top_init(st), kind as u8));
}

pub(super) fn env_object(bits: u64) {
    let _input_logging = input_record::Scope::enter();
    let st = state();
    if !st.env_rule {
        return host(Effect::HostRead, "env");
    }
    st.env_obj = bits;
    let top = top_init(st);
    if top != 0 && st.env_obj_last != top {
        st.env_obj_last = top;
        st.env_obs.push(EnvObs {
            init: top,
            kind: OBS_OBJECT,
            name: String::new(),
            value: None,
            ptr: 0,
            by: 0,
            by_pc: 0,
        });
    }
}

pub(super) fn env_storage(begin: usize, obj: usize) {
    let _input_logging = input_record::Scope::enter();
    let st = state();
    let h = obj - crate::gc::GC_HEADER_SIZE;
    let new: Vec<(usize, usize)> = st.births.as_slice()[begin.min(st.births.len)..]
        .iter()
        .map(|b| (b.addr, b.addr + b.size as usize))
        .collect();
    for (a, e) in new {
        st.env_storage.push((a, e));
        // The env object's header (class, shape, meta) is what dispatch and
        // identity read; its slots are the env block.
        let lo = if a == h {
            obj + std::mem::size_of::<crate::object::ObjectHeader>()
        } else {
            a + crate::gc::GC_HEADER_SIZE
        };
        watch_env_words(st, lo, e, ENV_BLOCK);
    }
}

pub(super) fn env_var(name: &str, value: Option<&str>, ptr: usize) {
    let _input_logging = input_record::Scope::enter();
    let st = state();
    input_record::event(st, "env_get", format!("{name:?}={value:?}"));
    let top = top_init(st);
    let site = st.env_site.take().filter(|s| s.0 == top).map(|s| s.1);
    if !st.env_rule {
        return host(Effect::HostRead, "env");
    }
    let mut kind = match (site, ptr) {
        (Some(1), 0) => OBS_STORE_UNSET,
        (Some(1), _) => OBS_STORE,
        _ => OBS_OBSERVE,
    };
    let idx = st.env_obs.len() as u32;
    if kind == OBS_STORE {
        let h = ptr - crate::gc::GC_HEADER_SIZE;
        match st.sizes.get(h as u64) {
            Some(size) => watch_env_words(st, ptr, h + size as usize, idx),
            // Not a logged birth: nothing to watch, so the value is observed.
            None => kind = OBS_OBSERVE,
        }
    }
    st.env_obs.push(EnvObs {
        init: top,
        kind,
        name: name.to_owned(),
        value: value.map(str::to_owned),
        ptr,
        by: 0,
        by_pc: 0,
    });
}

pub(super) fn env_has(name: &str, present: bool) {
    let _input_logging = input_record::Scope::enter();
    let st = state();
    input_record::event(st, "env_has", format!("{name:?}={present}"));
    if !st.env_rule {
        return host(Effect::HostRead, "env");
    }
    let top = top_init(st);
    st.env_obs.push(EnvObs {
        init: top,
        kind: OBS_HAS,
        name: name.to_owned(),
        value: present.then(|| "<set>".to_owned()),
        ptr: 0,
        by: 0,
        by_pc: 0,
    });
}

/// An access to a watched env word (`note_access`, signal context: no
/// allocation).
fn env_word_access(st: &mut State, v: u32, word: usize, write: bool, pc: usize) {
    let top = top_init(st);
    if v == POISON {
        if st.check_fail.len() < CHECK_FAIL_MAX {
            st.check_fail.push((top, word, pc, write));
            checkfail_line(st, top, word, pc, write);
        }
        return;
    }
    if top == 0 || st.inits[top as usize].in_d {
        return;
    }
    if v == ENV_BLOCK {
        // Enumeration or a write of the env block: rule 1.
        let i = &mut st.inits[top as usize];
        i.host_count += 1;
        i.in_d = true;
        if i.host.is_none_or(|h| h.class == Effect::TargetFact as u8) {
            i.host = Some(Hit {
                class: Effect::HostRead as u8,
                what: if write { "env_write" } else { "env_enum" },
                pcs: [pc, 0, 0, 0, 0, 0],
            });
        }
        if st.top_mode {
            unsafe { extend_top(st) };
        }
        set_mode_for_top(st);
        return;
    }
    if let Some(o) = st.env_obs.get_mut(v as usize) {
        // The word holding `capacity` and the `refcount` alias hint is the
        // string's ownership state, not its value: a store barrier demotes
        // the string to shared there (`js_string_addref`). The value words
        // (lengths, flags, bytes) are what an observation reads; the hint is
        // reported with the cell so the startup fill sets it the same way.
        if word == (o.ptr + std::mem::offset_of!(crate::StringHeader, refcount)) & !7 {
            return;
        }
        if o.kind == OBS_STORE {
            o.kind = OBS_STORE_READ;
            o.by = top;
            o.by_pc = pc;
        }
    }
}

/// The check run: the evaluation now on top is a member of D.
pub(super) fn skip_body() -> bool {
    let _input_logging = input_record::Scope::enter();
    let st = state();
    if st.skip.is_empty() {
        return false;
    }
    match st.frames.last() {
        Some(f) if f.kind == FrameKind::Init => {
            let i = &mut st.inits[f.init as usize];
            if !i.entry_module && st.skip.binary_search(&i.addr).is_ok() {
                let first = !i.skipped;
                i.skipped = true;
                if first {
                    // The imaged inits the census entered inside this body,
                    // evaluated here in entry order (the image holds them
                    // initialized: a later importer finds them done).
                    let addr = i.addr;
                    let kids: Vec<(usize, usize)> = st
                        .dkids
                        .iter()
                        .filter(|k| k.0 == addr)
                        .map(|k| (k.1, k.2))
                        .collect();
                    for (kid, guard) in kids {
                        crate::module_require::snap_evaluate_init(kid, guard);
                    }
                }
                return true;
            }
            false
        }
        _ => false,
    }
}

fn read_hex_lines(path: &str) -> Vec<usize> {
    std::fs::read_to_string(path)
        .map(|t| {
            t.lines()
                .filter_map(|l| {
                    usize::from_str_radix(l.split('\t').next()?.trim().trim_start_matches("0x"), 16)
                        .ok()
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A logical name for a module-global slot: its symbol (or the image
/// offset) from `dladdr`, never its address.
fn slot_name(slot: usize) -> String {
    unsafe {
        let mut info: libc::Dl_info = std::mem::zeroed();
        if libc::dladdr(slot as *const _, &mut info) != 0 {
            let base = info.dli_fbase as usize;
            if !info.dli_sname.is_null() && !info.dli_saddr.is_null() {
                let s = std::ffi::CStr::from_ptr(info.dli_sname).to_string_lossy();
                return format!("g:{s}+{}", slot - info.dli_saddr as usize);
            }
            return format!("g:+{:x}", slot - base);
        }
    }
    format!("g:?{slot:x}")
}

fn tsv_text(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c == '\t' || c == '\n' || c == '\r' {
                ' '
            } else {
                c
            }
        })
        .collect()
}

/// The `#cell`, `#guard`, `#knob` lines (design §2.10.8) and the per-init
/// slot owners for the check run.
/// The digest name of the env object and its storage.
const ENV_NAME: u64 = 0x454E_5600_0000_0000;

/// Whether `a` lies in mimalloc's heap memory (raw native pointers).
fn in_malloc(a: usize) -> bool {
    #[cfg(feature = "alloc-mimalloc")]
    {
        a >= 0x1_0000 && unsafe { libmimalloc_sys::mi_is_in_heap_region(a as *const _) }
    }
    #[cfg(not(feature = "alloc-mimalloc"))]
    {
        let _ = a;
        false
    }
}

fn write_env_report(st: &State, f: &mut std::fs::File, rank: &[u64]) {
    use std::io::Write;
    let in_d = |i: u32| st.inits[i as usize].in_d;
    let addr_of = |i: u32| st.inits[i as usize].addr;
    // Cells: every image word that holds the env object or a store-site
    // string. The image is the births of inits outside D and the global
    // slots they registered; the env block itself and words a member of D
    // wrote are not image words.
    let mut targets: std::collections::HashMap<u64, Vec<i64>> = Default::default();
    if st.env_obj != 0 {
        targets
            .entry(st.env_obj & 0xFFFF_FFFF_FFFF)
            .or_default()
            .push(-1);
    }
    // A var some imaged init observes is a guard member: the guard makes
    // every imaged string of it right, so its store-site reads need no cell.
    let guarded: std::collections::HashSet<&str> = st
        .env_obs
        .iter()
        .filter(|o| {
            matches!(
                o.kind,
                OBS_OBSERVE | OBS_HAS | OBS_STORE_UNSET | OBS_STORE_READ
            ) && (o.init == 0 || !in_d(o.init))
        })
        .map(|o| o.name.as_str())
        .collect();
    for (k, o) in st.env_obs.iter().enumerate() {
        if o.kind == OBS_STORE && !in_d(o.init) && !guarded.contains(o.name.as_str()) {
            targets
                .entry(o.ptr as u64 & 0xFFFF_FFFF_FFFF)
                .or_default()
                .push(k as i64);
        }
    }
    let mut found = vec![0u32; st.env_obs.len()];
    let mut env_obj_cells = 0u32;
    let mut cell = |what: &[i64], name: String, f: &mut std::fs::File| {
        for &t in what {
            if t < 0 {
                env_obj_cells += 1;
                let _ = writeln!(f, "#cell\tenv_object\t{name}\t\t");
            } else {
                let o = &st.env_obs[t as usize];
                found[t as usize] += 1;
                // The alias hint at the boundary (0 shared, 1 unique).
                let alias = unsafe { (*(o.ptr as *const crate::StringHeader)).refcount };
                let _ = writeln!(
                    f,
                    "#cell\tenv_var\t{name}\t{}\t{}\talias={alias}",
                    o.name, o.init
                );
            }
        }
    };
    let probe = |v: u64| -> Option<&Vec<i64>> {
        let hi = v >> 48;
        if hi == 0 || hi >= 0x7FF9 {
            targets.get(&(v & 0xFFFF_FFFF_FFFF))
        } else {
            None
        }
    };
    if !targets.is_empty() {
        let mut storage = st.env_storage.clone();
        storage.sort_unstable();
        let log = st.births.as_slice();
        for (k, b) in log.iter().enumerate() {
            if (b.init != 0 && in_d(b.init))
                || storage.binary_search_by_key(&b.addr, |r| r.0).is_ok()
            {
                continue;
            }
            let mut off = crate::gc::GC_HEADER_SIZE;
            while off + 8 <= b.size as usize {
                let word = b.addr + off;
                if !st.w.contains(word as u64) {
                    let v = unsafe { *(word as *const u64) };
                    if let Some(t) = probe(v) {
                        let t = t.clone();
                        cell(
                            &t,
                            format!("b:{:x}#{}+{}", addr_of(b.init), rank[k], off),
                            f,
                        );
                    }
                }
                off += 8;
            }
        }
        for (k, &slot) in st.globals_log.iter().enumerate() {
            let owner = st.slot_owner.get(k).copied().unwrap_or(0);
            if (owner != 0 && in_d(owner)) || st.w.contains(slot as u64) {
                continue;
            }
            let v = unsafe { *(slot as *const u64) };
            if let Some(t) = probe(v) {
                let t = t.clone();
                cell(&t, slot_name(slot), f);
            }
        }
    }
    let _ = writeln!(f, "#env_object_cells\t{env_obj_cells}");
    // The guard: every other observation by an imaged init (the runtime's
    // own, init 0, included: the IPC probe decides the process object).
    let mut seen: std::collections::HashSet<(String, u32, &'static str)> = Default::default();
    let mut guard = |name: &str,
                     value: Option<&str>,
                     init: u32,
                     consumer: &'static str,
                     by: (u32, usize),
                     f: &mut std::fs::File| {
        if init != 0 && in_d(init) {
            return;
        }
        if seen.insert((name.to_owned(), init, consumer)) {
            let _ = writeln!(
                f,
                "#guard\t{}\t{}\t{init}\t{:x}\t{consumer}\t{}\t{:x}",
                tsv_text(name),
                value.map_or("<unset>".to_owned(), tsv_text),
                addr_of(init),
                by.0,
                by.1
            );
        }
    };
    for (k, o) in st.env_obs.iter().enumerate() {
        match o.kind {
            OBS_OBSERVE => guard(&o.name, o.value.as_deref(), o.init, "observe", (0, 0), f),
            OBS_HAS => guard(&o.name, o.value.as_deref(), o.init, "has", (0, 0), f),
            OBS_STORE_UNSET => guard(&o.name, None, o.init, "store_unset", (0, 0), f),
            OBS_STORE_READ => guard(
                &o.name,
                o.value.as_deref(),
                o.init,
                "store_read",
                (o.by, o.by_pc),
                f,
            ),
            OBS_STORE if found[k] == 0 && !in_d(o.init) && !guarded.contains(o.name.as_str()) => {
                let _ = writeln!(f, "#cell_unstored\t{}\t{}", o.name, o.init);
            }
            _ => {}
        }
    }
    let program = input_record::program_env_names(st);
    for (init, name, value) in &st.env_reads {
        if (name.starts_with("PERRY_") || name.starts_with("RUST_")) && !program.contains(name) {
            let _ = writeln!(f, "#knob\t{name}\t{init}");
            continue;
        }
        let explained = st
            .env_obs
            .iter()
            .any(|o| o.init == *init && o.kind != OBS_OBJECT && o.name == *name);
        if !explained {
            guard(name, value.as_deref(), *init, "libc", (0, 0), f);
        }
    }
    let takers = st.env_obs.iter().filter(|o| o.kind == OBS_OBJECT).count();
    let _ = writeln!(
        f,
        "#env\trule={}\tq7={}\tobs={}\tobject_takers={takers}\tcheck={}",
        st.env_rule as u8,
        st.q7 as u8,
        st.env_obs.len(),
        !st.skip.is_empty() as u8
    );
    for &(init, word, pc, write) in &st.check_fail {
        let _ = writeln!(
            f,
            "#checkfail\tpoison\t{init}\t{:x}\t{}\t{pc:x}\t{}",
            addr_of(init),
            slot_name(word),
            write as u8
        );
    }
    if !st.skip.is_empty() {
        for (id, i) in st.inits.iter().enumerate().skip(1) {
            if i.in_d && !i.skipped && i.addr != ENTRY_BODY_ADDR && !i.entry_module {
                let _ = writeln!(f, "#checkfail\tjoined_d\t{id}\t{:x}", i.addr);
            }
        }
    }
    // The next check run's order: per member of D whose body the check skips,
    // the imaged inits entered inside it (directly or through members of D it
    // entered), in entry order.
    if st.skip.is_empty() {
        if let Ok(mut d) = std::fs::File::create(format!("{}.dkids", st.out)) {
            let skipped = |id: u32| {
                let i = &st.inits[id as usize];
                id != 0 && i.in_d && !i.entry_module && i.addr != ENTRY_BODY_ADDR && i.addr != 0
            };
            for (id, i) in st.inits.iter().enumerate().skip(1) {
                if in_d(id as u32) || i.addr == 0 || i.addr == ENTRY_BODY_ADDR {
                    continue;
                }
                let mut p = i.parent;
                while skipped(p) {
                    let _ = writeln!(d, "{:x}\t{:x}\t{:x}", addr_of(p), i.addr, i.guard);
                    p = st.inits[p as usize].parent;
                }
            }
        }
    }
    // The next check run's poison: the global slots members of D registered.
    if st.skip.is_empty() {
        if let Ok(mut d) = std::fs::File::create(format!("{}.dslots", st.out)) {
            for (k, &slot) in st.globals_log.iter().enumerate() {
                let owner = st.slot_owner.get(k).copied().unwrap_or(0);
                // The entry module runs in the check run: its slots are not poisoned.
                if owner != 0 && in_d(owner) && !st.inits[owner as usize].entry_module {
                    let _ = writeln!(d, "{slot:x}\t{owner}");
                }
            }
        }
    }
}

// Calling the kernel directly also covers libc callers that would otherwise
// read the vDSO without a syscall visible to the companion native trace.
#[no_mangle]
pub unsafe extern "C" fn clock_gettime(
    clock: libc::clockid_t,
    out: *mut libc::timespec,
) -> libc::c_int {
    let rc = libc::syscall(libc::SYS_clock_gettime, clock, out) as libc::c_int;
    let saved_errno = *libc::__errno_location();
    if inputs_on() && on_main_thread() {
        let _input_logging = input_record::Scope::enter();
        if rc == 0 && !out.is_null() {
            input_record::native_event(
                state(),
                "native.clock_gettime",
                format_args!(
                    "clock={clock};sec={};nsec={}",
                    (*out).tv_sec,
                    (*out).tv_nsec
                ),
            );
        } else {
            input_record::native_event(
                state(),
                "native.clock_gettime",
                format_args!("clock={clock};rc={rc};errno={saved_errno}"),
            );
        }
    }
    *libc::__errno_location() = saved_errno;
    rc
}

#[no_mangle]
pub unsafe extern "C" fn gettimeofday(
    out: *mut libc::timeval,
    zone: *mut libc::c_void,
) -> libc::c_int {
    let rc = libc::syscall(libc::SYS_gettimeofday, out, zone) as libc::c_int;
    let saved_errno = *libc::__errno_location();
    if inputs_on() && on_main_thread() {
        let _input_logging = input_record::Scope::enter();
        if rc == 0 && !out.is_null() {
            input_record::native_event(
                state(),
                "native.gettimeofday",
                format_args!("sec={};usec={}", (*out).tv_sec, (*out).tv_usec),
            );
        } else {
            input_record::native_event(
                state(),
                "native.gettimeofday",
                format_args!("rc={rc};errno={saved_errno}"),
            );
        }
    }
    *libc::__errno_location() = saved_errno;
    rc
}

#[no_mangle]
pub unsafe extern "C" fn time(out: *mut libc::time_t) -> libc::time_t {
    let rc = libc::syscall(libc::SYS_time, out) as libc::time_t;
    let saved_errno = *libc::__errno_location();
    if inputs_on() && on_main_thread() {
        let _input_logging = input_record::Scope::enter();
        input_record::native_event(
            state(),
            "native.time",
            format_args!("seconds={rc};errno={saved_errno}"),
        );
    }
    *libc::__errno_location() = saved_errno;
    rc
}

// Environment mutations have no syscall and must not disappear from native
// evidence. Resolve the libc operation afresh; this is instruments-only.
#[no_mangle]
pub unsafe extern "C" fn unsetenv(name: *const libc::c_char) -> libc::c_int {
    let saved_errno = *libc::__errno_location();
    if inputs_on() && on_main_thread() {
        let _input_logging = input_record::Scope::enter();
        let key = if name.is_null() {
            b"<null>".as_slice()
        } else {
            std::ffi::CStr::from_ptr(name).to_bytes()
        };
        let was = if c25() {
            if name.is_null() || getenv(name).is_null() {
                ";was=unset"
            } else {
                ";was=set"
            }
        } else {
            ""
        };
        input_record::native_event(
            state(),
            "native.unsetenv",
            format_args!("{}{was}", input_record::Hex(key)),
        );
        host(Effect::HostWrite, "env");
    }
    let next = libc::dlsym(libc::RTLD_NEXT, c"unsetenv".as_ptr());
    assert!(!next.is_null(), "libc unsetenv missing");
    let call: unsafe extern "C" fn(*const libc::c_char) -> libc::c_int = std::mem::transmute(next);
    *libc::__errno_location() = saved_errno;
    call(name)
}

#[no_mangle]
pub unsafe extern "C" fn setenv(
    name: *const libc::c_char,
    value: *const libc::c_char,
    overwrite: libc::c_int,
) -> libc::c_int {
    let saved_errno = *libc::__errno_location();
    if inputs_on() && on_main_thread() {
        let _input_logging = input_record::Scope::enter();
        let key = if name.is_null() {
            b"<null>".as_slice()
        } else {
            std::ffi::CStr::from_ptr(name).to_bytes()
        };
        let val = if value.is_null() {
            b"<null>".as_slice()
        } else {
            std::ffi::CStr::from_ptr(value).to_bytes()
        };
        input_record::native_event(
            state(),
            "native.setenv",
            format_args!(
                "{}={};overwrite={overwrite}{}",
                input_record::Hex(key),
                input_record::Hex(val),
                if !c25() {
                    ""
                } else if getenv(name).is_null() {
                    ";was=unset"
                } else {
                    ";was=set"
                }
            ),
        );
        host(Effect::HostWrite, "env");
    }
    let next = libc::dlsym(libc::RTLD_NEXT, c"setenv".as_ptr());
    assert!(!next.is_null(), "libc setenv missing");
    let call: unsafe extern "C" fn(
        *const libc::c_char,
        *const libc::c_char,
        libc::c_int,
    ) -> libc::c_int = std::mem::transmute(next);
    *libc::__errno_location() = saved_errno;
    call(name, value, overwrite)
}

// ---------------------------------------------------------------------------
// SNAP25: funnels, per-read argv, executable text (census only)
// ---------------------------------------------------------------------------

/// `PERRY_SNAP25=0` is the off arm: the census rows are exactly SNAP23's.
static C25: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub(super) fn c25() -> bool {
    C25.load(Relaxed)
}

#[thread_local]
static FUNNEL: std::cell::Cell<u8> = std::cell::Cell::new(0);
#[thread_local]
static ARGV_SITE: std::cell::Cell<(i32, i64)> = std::cell::Cell::new((-1, 0));

pub(super) fn funnel() -> u8 {
    FUNNEL.get()
}
pub(super) fn funnel_name(id: u8) -> &'static str {
    match id {
        1 => "exec_path",
        2 => "stack_maps",
        3 => "stack_bounds",
        4 => "parse",
        5 => "parallelism",
        6 => "gc",
        7 => "exec_argv",
        _ => "none",
    }
}
/// The syscall companion tags every later syscall of this thread with `id`.
fn funnel_marker(id: u8) {
    unsafe {
        let saved = *libc::__errno_location();
        libc::syscall(
            libc::SYS_lseek,
            -1 as libc::c_int,
            u64::from(id),
            0x5323 as libc::c_int,
        );
        *libc::__errno_location() = saved;
    }
}
pub(crate) struct FunnelScope(u8, bool);
impl FunnelScope {
    #[inline(always)]
    pub(crate) fn enter(id: u8) -> Self {
        if !(on() && c25()) {
            return Self(0, false);
        }
        let prev = FUNNEL.replace(id);
        funnel_marker(id);
        Self(prev, true)
    }
}
impl Drop for FunnelScope {
    #[inline(always)]
    fn drop(&mut self) {
        if self.1 {
            FUNNEL.set(self.0);
            funnel_marker(self.0);
        }
    }
}

pub(super) fn argv_site(kind: i32, index: i64) {
    ARGV_SITE.set((kind, index));
}

/// One `process.argv` read: `length=N`, `index=I;value=V` or `escape`.
pub(super) fn argv_read(value: f64) {
    let (kind, index) = ARGV_SITE.replace((-1, 0));
    if !(c25() && inputs_on() && on_main_thread()) {
        return;
    }
    let bits = value.to_bits();
    let arr = if bits >> 48 == 0x7ffd {
        (bits & 0x0000_ffff_ffff_ffff) as *mut crate::ArrayHeader
    } else {
        std::ptr::null_mut()
    };
    let text = {
        let _scope = input_record::Scope::enter();
        if arr.is_null() {
            format!("escape;site={kind};value=non_array:{bits:016x}")
        } else {
            let len = unsafe { (*arr).length };
            match kind {
                1 => format!("length={len}"),
                2 if index >= 0 && (index as u64) < u64::from(len) => {
                    let e = unsafe {
                        (crate::array::array_elements_ptr(arr) as *const f64)
                            .add(index as usize)
                            .read()
                            .to_bits()
                    };
                    let v = if e >> 48 == 0x7fff {
                        input_record::Value::input_value(
                            &((e & 0x0000_ffff_ffff_ffff) as *mut crate::StringHeader),
                        )
                    } else {
                        format!("bits:{e:016x}")
                    };
                    format!("index={index};value={v}")
                }
                2 => format!("index={index};value=undefined;length={len}"),
                _ => format!("escape;site={kind};length={len}"),
            }
        }
    };
    let _scope = input_record::Scope::enter();
    input_record::event(state(), "argv.read", text);
}

static EXE_TEXT: [std::sync::atomic::AtomicUsize; 3] = [
    std::sync::atomic::AtomicUsize::new(0),
    std::sync::atomic::AtomicUsize::new(0),
    std::sync::atomic::AtomicUsize::new(0),
];
/// (EXEC image, text lo, text hi) of the main executable: its executable
/// PT_LOAD; EXEC when the loader applied no bias (non-PIE).
pub(super) fn exe_text() -> (bool, usize, usize) {
    if EXE_TEXT[2].load(Relaxed) == 0 {
        unsafe extern "C" fn each(
            info: *mut libc::dl_phdr_info,
            _size: libc::size_t,
            data: *mut libc::c_void,
        ) -> libc::c_int {
            let out = &mut *(data as *mut (usize, usize, usize));
            let info = &*info;
            for i in 0..info.dlpi_phnum as usize {
                let ph = &*info.dlpi_phdr.add(i);
                if ph.p_type == libc::PT_LOAD && ph.p_flags & libc::PF_X != 0 {
                    let lo = info.dlpi_addr as usize + ph.p_vaddr as usize;
                    out.0 = usize::from(info.dlpi_addr == 0);
                    out.1 = lo;
                    out.2 = lo + ph.p_memsz as usize;
                    break;
                }
            }
            1 // the first object is the executable
        }
        let mut r = (0usize, 0usize, 0usize);
        unsafe { libc::dl_iterate_phdr(Some(each), &mut r as *mut _ as *mut libc::c_void) };
        EXE_TEXT[0].store(r.0, Relaxed);
        EXE_TEXT[1].store(r.1, Relaxed);
        EXE_TEXT[2].store(r.2.max(1), Relaxed);
    }
    (
        EXE_TEXT[0].load(Relaxed) == 1,
        EXE_TEXT[1].load(Relaxed),
        EXE_TEXT[2].load(Relaxed),
    )
}
