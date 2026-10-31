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

use super::Effect;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, Ordering::Relaxed};

const PAGE: usize = 4096;
const MAX_INITS: usize = 1 << 16;

/// The runner's own words, alone on their page: every tracked page that the
/// handler might protect is a page of JS state, never this one.
#[repr(C, align(4096))]
struct Anchor {
    on: AtomicBool,
    started: AtomicU8,
    state: AtomicPtr<State>,
    _pad: [u8; PAGE - 16],
}

static ANCHOR: Anchor = Anchor {
    on: AtomicBool::new(false),
    started: AtomicU8::new(0),
    state: AtomicPtr::new(std::ptr::null_mut()),
    _pad: [0; PAGE - 16],
};

#[inline(always)]
pub(super) fn on() -> bool {
    ANCHOR.on.load(Relaxed)
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
    assert!(p != libc::MAP_FAILED, "snap census: mmap {bytes} bytes failed");
    p.cast()
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
}

#[derive(Clone, Copy)]
struct Init {
    addr: usize,
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
    ir: u64,
    births: u64,
    birth_bytes: u64,
    host_count: u32,
    host: Option<Hit>,
    conflict: Option<Conflict>,
    in_d: bool,
    faults: u64,
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
}

/// Page flags (one byte per page).
const PF_W: u8 = 1;
const PF_R: u8 = 2;
const PF_PRE: u8 = 4;
const PF_LISTED: u8 = 8;
const PF_TRACKED: u8 = 16;

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
    out: String,
    inits: Vec<Init>,
    frames: Vec<Frame>,
    seq: u32,
    seeded: Vec<usize>,
    rule4: bool,
    sabotage_env: bool,
    entry_body_seq: u32,
    require_depth: i32,
    bookkeeping: i32,
    w: WordSet,
    r: WordSet,
    /// Registered module-global root slots (the handler cannot borrow
    /// `GLOBAL_ROOTS`: the interrupted code may hold it).
    globals: WordSet,
    /// One bit per 8-byte word of the reservation: a birth starts there.
    starts: *mut u64,
    /// Host touches while the runtime (init 0) works for an init: the realm
    /// populating itself on first use. (what, call chain), first per what.
    realm_hosts: Vec<(&'static str, [usize; 6])>,
    excluded: u64,
    /// Global root slots on no tracked page (the thread pointer's page,
    /// or not a binding: a root slot elsewhere): not watched.
    untracked_slots: u64,
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
    /// The main thread's static TLS block: module globals live there in a
    /// program with a Worker (per-thread init flags and bindings, #10399).
    tls: PageMap,
    /// Pages opened for the current instruction (the handler's own state is
    /// never in TLS: a TLS page may be protected).
    pending: [usize; 8],
    rset_word: usize,
    /// Handler depth: a fault inside the handler is reported, not hidden.
    in_handler: u32,
    /// The thread that runs module evaluation; the runner tracks only its
    /// memory (a worker agent has its own heap regions).
    main_tid: i64,
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
    /// Ground truth: every `getenv` by any code while an init runs.
    env_reads: Vec<(u32, String)>,
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
    unsafe { start(out) };
    true
}

unsafe fn start(out: String) {
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
    let mut fs: usize = 0;
    libc::syscall(libc::SYS_arch_prctl, 0x1003 /* ARCH_GET_FS */, &mut fs as *mut usize);
    // The executable's static TLS block sits right below the thread pointer
    // (x86_64 variant II): [fs - memsz rounded to its alignment, fs).
    let tls_size = executable_tls_size();
    // The page holding the thread pointer is never protected: the kernel
    // writes the thread's rseq area (in the TCB, right above it) on every
    // return to user space, and a protected page there kills the thread.
    let (tls_lo, tls_hi) = if fs != 0 && tls_size != 0 {
        // Whole pages only: the page below the block holds libc static TLS.
        ((fs - tls_size + PAGE - 1) & !(PAGE - 1), fs & !(PAGE - 1))
    } else {
        (fs, fs)
    };
    let mut st = Box::new(State {
        out,
        inits: Vec::with_capacity(4096),
        frames: Vec::with_capacity(256),
        seq: 0,
        seeded,
        rule4: std::env::var("PERRY_SNAP_RULE4").is_ok_and(|v| v == "1"),
        sabotage_env: std::env::var("PERRY_SNAP_SABOTAGE").is_ok_and(|v| v == "env"),
        entry_body_seq: 0,
        require_depth: 0,
        bookkeeping: 0,
        w: WordSet::new(28),
        r: WordSet::new(28),
        globals: WordSet::new(22),
        starts: map_zeroed::<u64>(((res_hi - res_lo) / 8).div_ceil(64).max(1)),
        realm_hosts: Vec::new(),
        excluded: 0,
        untracked_slots: 0,
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
        pending: [0; 8],
        rset_word: 0,
        in_handler: 0,
        main_tid: libc::syscall(libc::SYS_gettid),
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
    let st = state();
    // Nothing moves until the boundary.
    crate::gc::gc_suppress();
    // The objects that exist now are the runtime's (init 0).
    crate::arena::arena_walk_objects_with_block_index(|header, _| {
        let size = (*(header as *const crate::gc::GcHeader)).size as usize;
        set_start(st, header as usize);
        st.births.push(Birth {
            addr: header as usize,
            size: size as u32,
            init: 0,
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
        if page_flags(st, slot).is_none() {
            st.untracked_slots += 1;
        }
        st.globals.insert(slot as u64);
        st.globals_log.push(slot);
        mark_tracked(slot, slot + 8);
    }
    install_handlers();
    ANCHOR.on.store(true, Relaxed);
    crate::arena::refresh_inline_allocation_limit();
}

fn blank_init(addr: usize, parent: u32, reach: Reach) -> Init {
    Init {
        addr,
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
        ir: 0,
        births: 0,
        birth_bytes: 0,
        host_count: 0,
        host: None,
        conflict: None,
        in_d: false,
        faults: 0,
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
        unsafe { libc::syscall(libc::SYS_lseek, -1 as libc::c_int, top as libc::c_long, 0x5a) };
    }
    let want = top != 0 && st.inits[top].in_d;
    if want != st.d_mode {
        st.d_mode = want;
        unsafe { apply_protection(st) };
    }
}

pub(super) fn init_enter(addr: usize) {
    if !on() && !start_if_requested() {
        return;
    }
    let st = state();
    charge(st);
    let parent = st.frames.last().copied();
    let parent_id = parent.map_or(0, |f| f.init);
    let parent_in_body = match parent {
        None => st.entry_body_seq != 0,
        Some(f) if f.kind == FrameKind::Job => true,
        Some(f) => st.inits[f.init as usize].parts >= 2 || st.inits[f.init as usize].addr == 0,
    };
    let reach = if st.require_depth > 0 {
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
    st.seq += 1;
    let id = st.inits.len() as u32;
    assert!((id as usize) < MAX_INITS, "snap census: too many inits");
    let mut init = blank_init(addr, parent_id, reach);
    init.enter_seq = st.seq;
    init.open = true;
    let parent_dynamic = parent_id != 0 && st.inits[parent_id as usize].dynamic;
    init.dynamic = reach == Reach::DynImport || parent_dynamic;
    init.from_entry_body = (parent.is_none() && st.entry_body_seq != 0)
        || (parent_id != 0 && st.inits[parent_id as usize].from_entry_body);
    init.seeded = st.seeded.contains(&addr);
    init.in_d = init.seeded;
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
    set_mode_for_top(st);
}

pub(super) fn init_exit(threw: bool) {
    let st = state();
    charge(st);
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

pub(super) fn init_part() {
    let st = state();
    if let Some(f) = st.frames.last() {
        if f.kind == FrameKind::Init {
            let i = &mut st.inits[f.init as usize];
            i.parts = i.parts.saturating_add(1);
        }
    }
}

pub(super) fn require_depth(delta: i32) {
    state().require_depth += delta;
}

pub(super) fn bookkeeping_depth(delta: i32) {
    state().bookkeeping += delta;
}

pub(super) fn job_enter(owner: u32) {
    let st = state();
    charge(st);
    st.frames.push(Frame {
        init: owner,
        kind: FrameKind::Job,
    });
    set_mode_for_top(st);
}

pub(super) fn job_exit() {
    let st = state();
    charge(st);
    if st.frames.last().is_some_and(|f| f.kind == FrameKind::Job) {
        st.frames.pop();
    }
    set_mode_for_top(st);
}

pub(super) fn entry_body() {
    let st = state();
    if st.entry_body_seq == 0 {
        st.seq += 1;
        st.entry_body_seq = st.seq;
    }
}

pub(super) fn host(class: Effect, what: &'static str) {
    let st = state();
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
    if i.host.is_none() || (class != Effect::TargetFact && i.host.is_some_and(|h| h.class == 1))
    {
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
    let st = state();
    set_start(st, addr);
    let top = st.frames.last().map_or(0, |f| f.init);
    st.births.push(Birth {
        addr,
        size: size as u32,
        init: top,
    });
    let i = &mut st.inits[top as usize];
    i.births += 1;
    i.birth_bytes += size as u64;
}

pub(super) fn region_mapped(base: usize, len: usize) {
    let st = state();
    if unsafe { libc::syscall(libc::SYS_gettid) } != st.main_tid {
        return;
    }
    st.regions.push((base, base + len));
    mark_tracked(base, base + len);
    if st.d_mode {
        unsafe { libc::mprotect(base as *mut _, len, libc::PROT_NONE) };
    }
}

pub(super) fn global_root(slot: usize) {
    let st = state();
    if page_flags(st, slot).is_none() {
        st.untracked_slots += 1;
    }
    st.globals.insert(slot as u64);
    st.globals_log.push(slot);
    mark_tracked(slot, slot + 8);
    // Rule 2 names every binding a member of D initializes, written or not.
    let top = st.frames.last().map_or(0, |f| f.init) as usize;
    if top != 0 && st.inits[top].in_d {
        st.w.insert(slot as u64);
        flag_page(st, slot & !(PAGE - 1), PF_W);
    }
    let page = slot & !(PAGE - 1);
    if st.d_mode && page_flags(st, page).is_some() {
        unsafe { libc::mprotect(page as *mut _, PAGE, libc::PROT_NONE) };
    }
}

fn mark_tracked(lo: usize, hi: usize) {
    let st = state();
    let mut p = lo & !(PAGE - 1);
    while p < hi {
        if let Some(f) = page_flags(st, p) {
            unsafe { *f |= PF_TRACKED };
        }
        p += PAGE;
    }
}

#[inline]
fn page_flags(st: &State, addr: usize) -> Option<*mut u8> {
    st.heap.slot(addr).or_else(|| st.bin.slot(addr)).or_else(|| st.tls.slot(addr))
}

// ---------------------------------------------------------------------------
// Protection
// ---------------------------------------------------------------------------

fn policy(st: &State, page: usize) -> i32 {
    let Some(f) = page_flags(st, page) else {
        return libc::PROT_READ | libc::PROT_WRITE;
    };
    let f = unsafe { *f };
    if f & PF_TRACKED == 0 {
        return libc::PROT_READ | libc::PROT_WRITE;
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
    for m in [&st.bin, &st.tls] {
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
    if st.top_mode {
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
    libc::sigaction(libc::SIGSEGV, &a, std::ptr::addr_of_mut!(PREV_SEGV.0).cast());
    let mut t: libc::sigaction = std::mem::zeroed();
    t.sa_sigaction = on_trap as *const () as usize;
    t.sa_flags = libc::SA_SIGINFO | libc::SA_ONSTACK;
    libc::sigemptyset(&mut t.sa_mask);
    libc::sigaction(libc::SIGTRAP, &t, std::ptr::addr_of_mut!(PREV_TRAP.0).cast());
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
            let uc = ctx as *mut libc::ucontext_t;
            let pc = (*uc).uc_mcontext.gregs[libc::REG_RIP as usize] as u64;
            diag(&[0x7d, tid as u64, addr as u64, pc]);
            return chain(std::ptr::addr_of!(PREV_SEGV.0).cast(), sig);
        }
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
        if !tracked || policy(st, page) == libc::PROT_READ | libc::PROT_WRITE {
            let uc = ctx as *mut libc::ucontext_t;
            let pc = (*uc).uc_mcontext.gregs[libc::REG_RIP as usize] as u64;
            let flags = page_flags(st, page).map_or(0xff, |f| *f as u64);
            let range = if st.heap.slot(addr).is_some() { 1 } else if st.bin.slot(addr).is_some() { 2 } else if st.tls.slot(addr).is_some() { 3 } else { 0 };
            diag(&[addr as u64, pc, flags, range, st.d_mode as u64, st.top_mode as u64]);
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
        note_access(st, addr & !7, write, pc);
        gregs[libc::REG_EFL as usize] |= EFLAGS_TF;
    }
}

extern "C" fn on_trap(sig: libc::c_int, _info: *mut libc::siginfo_t, ctx: *mut libc::c_void) {
    unsafe {
        let st_ptr = ANCHOR.state.load(Relaxed);
        if st_ptr.is_null() || (*st_ptr).pending[0] == 0 {
            return chain(std::ptr::addr_of!(PREV_TRAP.0).cast(), sig);
        }
        let st = &mut *st_ptr;
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
            for &page in pending.iter().take_while(|p| **p != 0) {
                libc::mprotect(page as *mut _, PAGE, policy(st, page));
            }
        }
        st.pending = [0; 8];
        let uc = ctx as *mut libc::ucontext_t;
        (*uc).uc_mcontext.gregs[libc::REG_EFL as usize] &= !EFLAGS_TF;
    }
}

/// One access to a tracked word by the top frame.
fn note_access(st: &mut State, word: usize, write: bool, pc: usize) {
    if st.bookkeeping > 0 {
        st.faults_ignored += 1;
        return;
    }
    // Words of the binary range count only as global slots or realm cells.
    if (st.bin.slot(word).is_some() || st.tls.slot(word).is_some())
        && !(word >= st.realm.0 && word < st.realm.1)
        && !st.globals.contains(word as u64)
    {
        st.faults_ignored += 1;
        return;
    }
    // Immutable value-identity cells are not locations: a string never
    // changes after its birth (its cached hash is no observation), and a
    // shared canonical key list is an interned trie node. Reaching one
    // through a location a member of D wrote is caught at that location.
    if st.heap.slot(word).is_some() {
        if let Some(h) = object_start(st, word) {
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
                return;
            }
        }
    }
    let top = st.frames.last().map_or(0, |f| f.init) as usize;
    st.inits[top].faults += 1;
    let w = word as u64;
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
            });
        }
        i.in_d = true;
        unsafe { extend_top(st) };
        st.d_mode = true;
        unsafe { apply_protection(st) };
        return;
    }
    if st.d_mode {
        if write {
            st.w.insert(w);
            flag_page(st, word & !(PAGE - 1), PF_W);
        } else if !st.w.contains(w) {
            st.r.insert(w);
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
    } else if write
        && st.rule4_armed
        && st.inits[top].dynamic
        && pre_existing(st, word)
    {
        Some(4)
    } else {
        None
    };
    if let Some(rule) = rule {
        let i = &mut st.inits[top];
        if i.conflict.is_none() {
            i.conflict = Some(Conflict {
                rule,
                word,
                pc,
                write,
            });
        }
        i.in_d = true;
        st.d_mode = true;
        unsafe { apply_protection(st) };
        // This access is the member's first recorded one.
        if write {
            st.w.insert(w);
            flag_page(st, word & !(PAGE - 1), PF_W);
        } else {
            st.r.insert(w);
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
    if on() {
        let st = state();
        let top = st.frames.last().map_or(0, |f| f.init);
        if top != 0 && st.env_reads.len() < 1 << 20 {
            st.env_reads.push((top, String::from_utf8_lossy(key).into_owned()));
        }
    }
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
    let st = state();
    if st.boundary_seq != 0 {
        return;
    }
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
    let mut top: Vec<usize> = (0..st.cand.len()).filter(|&i| st.cand_writes[i] > 0).collect();
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
        let _ = writeln!(f, "#unattributed\t{}\t{}", st.live_unattributed[0], st.live_unattributed[1]);
        let mut rows: Vec<_> = st.live_by.iter().collect();
        rows.sort();
        for ((id, t), row) in rows {
            let name = crate::gc::gc_type_info(*t).map_or("?", |i| i.name);
            let _ = writeln!(f, "{id}\t{name}\t{}\t{}", row[0], row[1]);
        }
    }
}

fn write_report(st: &mut State) {
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
        "#id\taddr\tparent\treach\tdynamic\tfrom_entry_body\tenter\texit\topen\tthrew\tseeded\tin_d\tir\tbirths\tbirth_bytes\tfaults\thost_count\thost_class\thost_what\thost_pcs\tconflict_rule\tconflict_word\tconflict_write\tconflict_pc\tconflict_owner"
    );
    let births = {
        let mut v: Vec<(usize, usize, u32)> = st
            .births
            .as_slice()
            .iter()
            .map(|b| (b.addr, b.addr + b.size as usize, b.init))
            .collect();
        v.sort_unstable();
        v
    };
    let owner = |word: usize| -> String {
        let i = births.partition_point(|&(a, _, _)| a <= word);
        if i > 0 && word < births[i - 1].1 {
            format!("birth:{}@{:x}", births[i - 1].2, births[i - 1].0)
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
        let (cr, cw, cwr, cpc, co) = match i.conflict {
            Some(c) => (
                c.rule.to_string(),
                format!("{:x}", c.word),
                (c.write as u8).to_string(),
                format!("{:x}", c.pc),
                owner(c.word),
            ),
            None => Default::default(),
        };
        let _ = writeln!(
            f,
            "{id}\t{:x}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{hc}\t{hw}\t{hp}\t{cr}\t{cw}\t{cwr}\t{cpc}\t{co}",
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
        );
    }
    // Step 6: a digest of the imaged heap per init. A birth is named by its
    // init's evaluation address and its rank among that init's births; a
    // word that holds a pointer into the reservation hashes as the name of
    // the object it points into (plus the offset), a word a member of D
    // wrote is left out (the image holds it as it was before), any other
    // word hashes as itself.
    {
        let log = st.births.as_slice();
        let mut rank: std::collections::HashMap<u32, u64> = std::collections::HashMap::new();
        let mut names: Vec<(usize, usize, u64)> = Vec::with_capacity(log.len());
        for b in log {
            let k = rank.entry(b.init).or_insert(0);
            let a = st.inits[b.init as usize].addr as u64;
            names.push((b.addr, b.addr + b.size as usize, a.wrapping_mul(0x100000001B3) ^ *k));
            *k += 1;
        }
        names.sort_unstable();
        let name_of = |v: usize| -> Option<(u64, usize)> {
            let i = names.partition_point(|&(a, _, _)| a <= v);
            (i > 0 && v < names[i - 1].1).then(|| (names[i - 1].2, v - names[i - 1].0))
        };
        let mut per: std::collections::BTreeMap<usize, (u64, u64, u64)> = Default::default();
        let mix = |h: &mut u64, x: u64| {
            *h ^= x;
            *h = h.wrapping_mul(0x100000001B3);
        };
        for b in log {
            if b.init != 0 && st.inits[b.init as usize].in_d {
                continue;
            }
            let e = per
                .entry(st.inits[b.init as usize].addr)
                .or_insert((0xcbf29ce484222325, 0, 0));
            e.1 += 1;
            let mut off = 0;
            while off + 8 <= b.size as usize {
                let word = b.addr + off;
                off += 8;
                if st.w.contains(word as u64) {
                    continue;
                }
                let v = unsafe { *(word as *const u64) };
                e.2 += 1;
                let hi = v >> 48;
                if hi == 0 || hi >= 0x7FF9 {
                    if let Some((n, o)) = name_of((v & 0xFFFF_FFFF_FFFF) as usize) {
                        mix(&mut e.0, hi ^ 0xA5A5);
                        mix(&mut e.0, n);
                        mix(&mut e.0, o as u64);
                        continue;
                    }
                }
                mix(&mut e.0, v);
            }
        }
        let mut total = 0xcbf29ce484222325u64;
        for (addr, (h, objs, words)) in &per {
            mix(&mut total, *h);
            let _ = writeln!(f, "#digest\t{addr:x}\t{h:016x}\t{objs}\t{words}");
        }
        let _ = writeln!(f, "#digest_total\t{total:016x}\t{}", per.len());
    }
    let _ = writeln!(f, "#excluded\t{}", st.excluded);
    let _ = writeln!(f, "#untracked_slots\t{}\tof\t{}", st.untracked_slots, st.globals.len);
    for (what, pcs) in &st.realm_hosts {
        let pcs: Vec<String> = pcs.iter().map(|p| format!("{p:x}")).collect();
        let _ = writeln!(f, "#realm_host\t{what}\t{}", pcs.join(","));
    }
    for (init, name) in &st.env_reads {
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
