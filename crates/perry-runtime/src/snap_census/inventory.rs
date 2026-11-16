//! Cut-point inventory (`PERRY_SNAP_INVENTORY_BEFORE=<init n>`): when the
//! runner is about to enter init `n`, write `<census out>.inventory.tsv`: what
//! an image cut at that point would have to hold.
//!
//! Rows (tab separated; addresses hex):
//! - `#inventory before_init=N holds=collected|not-collected(no-hold)`;
//! - `cell <type> <name> <form> <birth init|-> <count> <bytes>`: every cell
//!   the allocators hold, per GC type and birth init (`-`: no birth record).
//!   The census run suppresses collection, so the walk is the cell set
//!   without a mark: an upper bound of the reachable set. `form` is `cell`
//!   (the layout describes every pointer and no native payload: an image
//!   form exists), `native` (a native payload: no image form) or `none`
//!   (a type id the runtime does not know);
//! - `word <data|tls-program|tls-runtime> <address> <tls offset> <old> <new>
//!   <class>`: every 8-byte word of the binary's writable data/bss and of the
//!   main thread's static TLS block that differs from its value when the
//!   census started. `class` is `gc` (points into a cell born by the
//!   program), `realm` (a cell born by the runtime), `image` (linker-owned
//!   immutable data), `tls` (the TLS block), `static` (elsewhere in the
//!   binary), `malloc` (a pointer-shaped value outside all of those) or
//!   `scalar`. Symbols are attached offline (`scripts/snap_inputs/check_inv.py`);
//! - `#total ...` lines with the sums;
//! - with `PERRY_SNAP_INVENTORY_LIVE=1`: a reachability mark from the roots
//!   the collector's full trace reads (stack maps, module globals, runtime
//!   scanners), through the collector's own traced-slot enumeration. The mark
//!   bits are a local bitset of the call, dropped after it; no collector flag
//!   is set. Rows: `live <type> <name> <form> <birth init> <n> <bytes>`,
//!   `garbage <type> <name> <n> <bytes>`, `root_src <source> <n> <bytes>`
//!   (cells first reached from that source), `slot_target <class> <n>` (every
//!   traced slot of a live cell), `special <type> <name> <live|garbage>
//!   <birth init> <holder type> <holder birth init> <n>` for interpreted
//!   functions and native cells, `handle_holder <type> <birth init> <n>`
//!   (live cells holding a handle-band id), and `#live`/`#garbage` totals.
//!   A malloc-tracked cell's birth init is the init that was on top when its
//!   `objects` slot was appended (the census holds collection, so the list
//!   only grows): the runner reports every change of the top init.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering::Relaxed};

unsafe extern "C" {
    static __data_start: u8;
    static _end: u8;
    static __executable_start: u8;
    static __start_perry_strings: u8;
    static __stop_perry_strings: u8;
    static __start_perry_layouts: u8;
    static __stop_perry_layouts: u8;
    static __start_perry_atoms: u8;
    static __stop_perry_atoms: u8;
}

/// What the census saw when it started: the writable data/bss and the main
/// thread's TLS block (a copy), and the init the report is written before.
struct Baseline {
    before: u32,
    data: (usize, Vec<u8>),
    tls: (usize, Vec<u8>),
    /// The program's TLS slot ranges (`tls_program`): data, bss.
    program_tls: [usize; 4],
    /// Ranges of the census's own words, never reported.
    skip: Vec<(usize, usize)>,
}

static BASELINE: AtomicPtr<Baseline> = AtomicPtr::new(std::ptr::null_mut());
static LIVE_ON: AtomicBool = AtomicBool::new(false);

/// The top init over the malloc-tracked object list: `(list length when the
/// init became top, init)`, in order; `reliable` stays true while the list
/// never shrinks.
struct Trans {
    owner: Option<std::thread::ThreadId>,
    v: Vec<(usize, u32)>,
    reliable: bool,
}
static TRANS: std::sync::Mutex<Trans> = std::sync::Mutex::new(Trans {
    owner: None,
    v: Vec::new(),
    reliable: true,
});

/// The runner's top init changed (a frame was pushed or popped).
pub(super) fn frame_changed(top: u32) {
    if !LIVE_ON.load(Relaxed) {
        return;
    }
    let Some(n) = crate::gc::MALLOC_STATE.with(|s| s.try_borrow().map(|s| s.objects.len()).ok())
    else {
        return;
    };
    let mut t = TRANS.lock().unwrap_or_else(|e| e.into_inner());
    if t.owner != Some(std::thread::current().id()) {
        return;
    }
    if t.v.last().is_some_and(|x| x.1 == top) {
        return;
    }
    if t.v.last().is_some_and(|x| n < x.0) {
        t.reliable = false;
    }
    t.v.push((n, top));
}

fn copy(lo: usize, hi: usize) -> (usize, Vec<u8>) {
    // SAFETY: `[lo, hi)` is mapped writable data of this process.
    (
        lo,
        unsafe { std::slice::from_raw_parts(lo as *const u8, hi - lo) }.to_vec(),
    )
}

/// The census started: remember the words the report compares against.
/// `anchor` is the runner's own page, `tls_block` the main thread's static
/// TLS block, `program_tls` the program's anchored TLS ranges.
pub(super) fn start(anchor: (usize, usize), tls_block: (usize, usize), program_tls: [usize; 4]) {
    let Some(before) = std::env::var("PERRY_SNAP_INVENTORY_BEFORE")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
    else {
        return;
    };
    if std::env::var("PERRY_SNAP_INVENTORY_LIVE").is_ok_and(|v| v == "1") {
        TRANS.lock().unwrap_or_else(|e| e.into_inner()).owner = Some(std::thread::current().id());
        LIVE_ON.store(true, Relaxed);
    }
    let lo = std::ptr::addr_of!(__data_start) as usize;
    let hi = std::ptr::addr_of!(_end) as usize;
    let own = std::ptr::addr_of!(BASELINE) as usize;
    let b = Box::new(Baseline {
        before,
        data: copy(lo, hi),
        tls: copy(tls_block.0, tls_block.1),
        program_tls,
        skip: vec![anchor, (own, own + 8)],
    });
    BASELINE.store(Box::into_raw(b), Relaxed);
}

/// Image form of a GC type (design: cells with a layout that names every
/// pointer are imageable; native payloads have no image form).
fn form(obj_type: u8) -> &'static str {
    use crate::gc::*;
    if gc_type_info(obj_type).is_none() {
        return "none";
    }
    match obj_type {
        GC_TYPE_NATIVE_ARENA_OWNER
        | GC_TYPE_NATIVE_TYPED_VIEW
        | GC_TYPE_NATIVE_HANDLE
        | GC_TYPE_NATIVE_POD_VIEW
        | GC_TYPE_TEMPORAL
        | GC_TYPE_REGEX_PROGRAM
        | GC_TYPE_REGEX_CACHE
        | GC_TYPE_INTERP_FUNCTION
        | GC_TYPE_BUFFER_SECRET_KEY
        | GC_TYPE_BUFFER_CRYPTO_KEY => "native",
        _ => "cell",
    }
}

struct Ctx {
    cells: Vec<(usize, usize, u32)>,
    images: [(usize, usize); 3],
    tls: (usize, usize),
    exe: (usize, usize),
}

fn class(v: u64, c: &Ctx) -> &'static str {
    let a = if (v >> 48) >= 0x7ff8 {
        v & 0xffff_ffff_ffff
    } else {
        v
    } as usize;
    let within = |r: (usize, usize)| a >= r.0 && a < r.1;
    if within(c.tls) {
        return "tls";
    }
    if c.images.iter().any(|r| within(*r)) {
        return "image";
    }
    let i = c.cells.partition_point(|x| x.0 <= a);
    if i > 0 && a < c.cells[i - 1].0 + c.cells[i - 1].1 {
        return if c.cells[i - 1].2 == 0 { "realm" } else { "gc" };
    }
    if within(c.exe) {
        return "static";
    }
    if a >= 0x10000 && a < (1 << 47) {
        return "malloc";
    }
    "scalar"
}

/// The runner reaches init `id`. `birth(addr)` names the init that allocated
/// the cell holding `addr`; `holds` is whether the run holds collection.
pub(super) fn at_init(id: u32, out: &str, holds: bool, birth: &dyn Fn(usize) -> Option<u32>) {
    let p = BASELINE.load(Relaxed);
    if p.is_null() {
        return;
    }
    // SAFETY: leaked in `start`, never freed.
    let b = unsafe { &*p };
    if b.before != id {
        return;
    }
    BASELINE.store(std::ptr::null_mut(), Relaxed);
    let _book = super::bookkeeping();
    let mut f = match std::fs::File::create(format!("{out}.inventory.tsv")) {
        Ok(f) => std::io::BufWriter::new(f),
        Err(_) => return,
    };
    let _ = writeln!(
        f,
        "#inventory\tbefore_init={id}\tholds={}",
        if holds {
            "collected"
        } else {
            "not-collected(no-hold)"
        }
    );
    // Cells: the arena walk and the malloc-tracked objects, minus free slots.
    let free: HashSet<usize> = crate::gc::ARENA_FREE_LIST
        .with(|fl| fl.borrow().iter().map(|&(ptr, _)| ptr as usize).collect());
    let mut headers: Vec<*mut u8> = Vec::new();
    crate::arena::arena_walk_objects(|h| headers.push(h));
    let malloc: Vec<*mut crate::gc::GcHeader> =
        crate::gc::MALLOC_STATE.with(|s| s.borrow().objects.clone());
    let malloc_index: HashMap<usize, usize> = malloc
        .iter()
        .enumerate()
        .map(|(i, &h)| (h as usize, i))
        .collect();
    headers.extend(malloc.into_iter().map(|h| h as *mut u8));
    let mut live_cells: Vec<LiveCell> = Vec::new();
    let mut cells: Vec<(usize, usize, u32)> = Vec::new();
    let mut rows: BTreeMap<(u8, i64), [u64; 2]> = BTreeMap::new();
    for h in headers {
        // SAFETY: headers of cells the allocators hold.
        let (t, size, flags) = unsafe {
            let g = &*(h as *const crate::gc::GcHeader);
            (g.obj_type, g.size as usize, g.gc_flags)
        };
        let a = h as usize;
        if t == 0 || t > crate::gc::GC_TYPE_MAX || size == 0 {
            continue;
        }
        if flags & crate::gc::GC_FLAG_FORWARDED != 0
            || free.contains(&a)
            || free.contains(&(a + crate::gc::GC_HEADER_SIZE))
        {
            continue;
        }
        let init = birth(a).map_or(-1, i64::from);
        cells.push((a, size, init.max(0) as u32));
        live_cells.push(LiveCell {
            a,
            size,
            t,
            birth: init.max(0) as u32,
            malloc_slot: malloc_index.get(&a).copied(),
        });
        let r = rows.entry((t, init)).or_insert([0; 2]);
        r[0] += 1;
        r[1] += size as u64;
    }
    let mut by_type: BTreeMap<u8, [u64; 2]> = BTreeMap::new();
    for ((t, init), r) in &rows {
        let name = crate::gc::gc_type_info(*t).map_or("?", |i| i.name);
        let init = if *init < 0 {
            "-".to_owned()
        } else {
            init.to_string()
        };
        let _ = writeln!(
            f,
            "cell\t{t}\t{name}\t{}\t{init}\t{}\t{}",
            form(*t),
            r[0],
            r[1]
        );
        let s = by_type.entry(*t).or_insert([0; 2]);
        s[0] += r[0];
        s[1] += r[1];
    }
    let (mut cell_n, mut cell_b, mut nat_n, mut nat_b) = (0u64, 0u64, 0u64, 0u64);
    for (t, r) in &by_type {
        let name = crate::gc::gc_type_info(*t).map_or("?", |i| i.name);
        let _ = writeln!(f, "#type\t{t}\t{name}\t{}\t{}\t{}", form(*t), r[0], r[1]);
        if form(*t) == "cell" {
            cell_n += r[0];
            cell_b += r[1];
        } else {
            nat_n += r[0];
            nat_b += r[1];
        }
    }
    cells.sort_unstable();
    let sect = |lo: &u8, hi: &u8| (lo as *const u8 as usize, hi as *const u8 as usize);
    // SAFETY: linker-defined section bounds.
    let ctx = unsafe {
        Ctx {
            cells,
            images: [
                sect(&__start_perry_strings, &__stop_perry_strings),
                sect(&__start_perry_layouts, &__stop_perry_layouts),
                sect(&__start_perry_atoms, &__stop_perry_atoms),
            ],
            tls: (b.tls.0, b.tls.0 + b.tls.1.len()),
            exe: sect(&__executable_start, &_end),
        }
    };
    if LIVE_ON.load(Relaxed) {
        live_report(&mut f, live_cells, &ctx.images, holds);
    }
    // Changed words, data/bss then TLS.
    let mut counts: BTreeMap<(&str, &str), u64> = BTreeMap::new();
    for (region, (lo, old)) in [("data", &b.data), ("tls", &b.tls)] {
        for (i, w) in old.chunks_exact(8).enumerate() {
            let a = lo + i * 8;
            if b.skip.iter().any(|s| a >= s.0 && a < s.1) {
                continue;
            }
            // SAFETY: the same mapped range the baseline copied.
            let now = unsafe { std::ptr::read_volatile(a as *const u64) };
            let was = u64::from_le_bytes(w.try_into().unwrap());
            if now == was {
                continue;
            }
            let region = if region == "tls" {
                let p = b.program_tls;
                if (a >= p[0] && a < p[1]) || (a >= p[2] && a < p[3]) {
                    "tls-program"
                } else {
                    "tls-runtime"
                }
            } else {
                region
            };
            let cl = class(now, &ctx);
            let off = if region == "data" { 0 } else { a - lo };
            let _ = writeln!(f, "word\t{region}\t{a:x}\t{off:x}\t{was:x}\t{now:x}\t{cl}");
            *counts.entry((region, cl)).or_insert(0) += 1;
        }
    }
    for ((region, cl), n) in &counts {
        let _ = writeln!(f, "#words\t{region}\t{cl}\t{n}");
    }
    let _ = writeln!(
        f,
        "#total\tcell_n={cell_n}\tcell_bytes={cell_b}\tnative_n={nat_n}\tnative_bytes={nat_b}\twords={}",
        counts.values().sum::<u64>()
    );
    let _ = f.flush();
}

struct LiveCell {
    a: usize,
    size: usize,
    t: u8,
    birth: u32,
    /// Index in the malloc-tracked object list, if the cell is in it.
    malloc_slot: Option<usize>,
}

enum Target {
    Cell(usize),
    Image,
    Handle,
    Untracked,
    Null,
    Other,
}

fn find(cells: &[LiveCell], a: usize) -> Option<usize> {
    let i = cells.partition_point(|c| c.a <= a);
    (i > 0 && a < cells[i - 1].a + cells[i - 1].size).then(|| i - 1)
}

fn decode(bits: u64, cells: &[LiveCell], images: &[(usize, usize); 3]) -> Target {
    use crate::value::{
        BIGINT_TAG, HANDLE_TAG, JS_HANDLE_TAG, POINTER_MASK, POINTER_TAG, STRING_TAG, TAG_MASK,
    };
    let a = match bits & TAG_MASK {
        POINTER_TAG | STRING_TAG | BIGINT_TAG => (bits & POINTER_MASK) as usize,
        HANDLE_TAG | JS_HANDLE_TAG => return Target::Handle,
        0 => bits as usize,
        _ => return Target::Other,
    };
    if a == 0 {
        return Target::Null;
    }
    if crate::value::addr_class::is_handle_band(a) {
        return Target::Handle;
    }
    if images.iter().any(|r| a >= r.0 && a < r.1) {
        return Target::Image;
    }
    if let Some(i) = find(cells, a) {
        return Target::Cell(i);
    }
    if a >= 0x10000 && a < (1 << 47) {
        return Target::Untracked;
    }
    Target::Other
}

/// The mark and its rows (see the module comment).
fn live_report(
    f: &mut impl Write,
    mut cells: Vec<LiveCell>,
    images: &[(usize, usize); 3],
    holds: bool,
) {
    cells.sort_unstable_by_key(|c| c.a);
    let n = cells.len();
    // Birth init of a malloc-tracked cell: the top init when its list slot
    // was appended.
    let (trans, reliable) = {
        let t = TRANS.lock().unwrap_or_else(|e| e.into_inner());
        (t.v.clone(), t.reliable && holds)
    };
    for c in cells.iter_mut() {
        if let Some(slot) = c.malloc_slot {
            let k = trans.partition_point(|x| x.0 <= slot);
            c.birth = if k > 0 { trans[k - 1].1 } else { 0 };
        }
    }
    let mut roots: Vec<(&'static str, u64)> = Vec::new();
    crate::gc::census_visit_roots(&mut |source, bits| {
        roots.push((source, bits));
    });
    let mut marked = vec![0u64; n.div_ceil(64)];
    let mut holder = vec![u32::MAX; n];
    let mut work: Vec<usize> = Vec::new();
    let mut by_source: BTreeMap<&'static str, [u64; 2]> = BTreeMap::new();
    let mut slots: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut handle_holders: BTreeMap<(u8, u32), u64> = BTreeMap::new();
    for (source, bits) in roots {
        if let Target::Cell(i) = decode(bits, &cells, images) {
            if marked[i / 64] >> (i % 64) & 1 == 1 {
                continue;
            }
            marked[i / 64] |= 1 << (i % 64);
            work.push(i);
        }
        let r = by_source.entry(source).or_insert([0; 2]);
        while let Some(i) = work.pop() {
            r[0] += 1;
            r[1] += cells[i].size as u64;
            let mut found: Vec<u64> = Vec::new();
            // SAFETY: a cell the allocators hold; the slots are read in place.
            unsafe {
                crate::gc::visit_traced_slots(cells[i].a as *mut crate::gc::GcHeader, |slot| {
                    found.push(std::ptr::read_volatile(slot as *const u64));
                });
            }
            for w in found {
                let class = match decode(w, &cells, images) {
                    Target::Cell(j) => {
                        if marked[j / 64] >> (j % 64) & 1 == 0 {
                            marked[j / 64] |= 1 << (j % 64);
                            holder[j] = i as u32;
                            work.push(j);
                        }
                        if cells[j].birth == 0 {
                            "realm_cell"
                        } else if cells[j].malloc_slot.is_some() {
                            "malloc_cell"
                        } else {
                            "arena_cell"
                        }
                    }
                    Target::Image => "image",
                    Target::Handle => {
                        *handle_holders
                            .entry((cells[i].t, cells[i].birth))
                            .or_insert(0) += 1;
                        "handle_id"
                    }
                    Target::Untracked => "untracked_malloc",
                    Target::Null => "null",
                    Target::Other => "other",
                };
                *slots.entry(class).or_insert(0) += 1;
            }
        }
    }
    let name = |t: u8| crate::gc::gc_type_info(t).map_or("?", |i| i.name);
    let mut live: BTreeMap<(u8, u32), [u64; 2]> = BTreeMap::new();
    let mut garbage: BTreeMap<u8, [u64; 2]> = BTreeMap::new();
    let mut special: BTreeMap<(u8, bool, u32, u8, u32), u64> = BTreeMap::new();
    let (mut live_n, mut live_b, mut garb_n, mut garb_b) = (0u64, 0u64, 0u64, 0u64);
    let mut by_form: BTreeMap<&str, [u64; 2]> = BTreeMap::new();
    for (i, c) in cells.iter().enumerate() {
        let is_live = marked[i / 64] >> (i % 64) & 1 == 1;
        if is_live {
            let r = live.entry((c.t, c.birth)).or_insert([0; 2]);
            r[0] += 1;
            r[1] += c.size as u64;
            live_n += 1;
            live_b += c.size as u64;
            let r = by_form.entry(form(c.t)).or_insert([0; 2]);
            r[0] += 1;
            r[1] += c.size as u64;
        } else {
            let r = garbage.entry(c.t).or_insert([0; 2]);
            r[0] += 1;
            r[1] += c.size as u64;
            garb_n += 1;
            garb_b += c.size as u64;
        }
        use crate::gc::*;
        if matches!(
            c.t,
            GC_TYPE_INTERP_FUNCTION
                | GC_TYPE_NATIVE_HANDLE
                | GC_TYPE_NATIVE_TYPED_VIEW
                | GC_TYPE_NATIVE_POD_VIEW
                | GC_TYPE_NATIVE_ARENA_OWNER
        ) {
            let (ht, hb) = match holder[i] {
                u32::MAX => (0, 0),
                h => (cells[h as usize].t, cells[h as usize].birth),
            };
            *special.entry((c.t, is_live, c.birth, ht, hb)).or_insert(0) += 1;
        }
    }
    let _ = writeln!(
        f,
        "#live_birth\tmalloc_births={}",
        if reliable { "tracked" } else { "unreliable" }
    );
    for ((t, init), r) in &live {
        let _ = writeln!(
            f,
            "live\t{t}\t{}\t{}\t{init}\t{}\t{}",
            name(*t),
            form(*t),
            r[0],
            r[1]
        );
    }
    for (t, r) in &garbage {
        let _ = writeln!(f, "garbage\t{t}\t{}\t{}\t{}", name(*t), r[0], r[1]);
    }
    for (s, r) in &by_source {
        let _ = writeln!(f, "root_src\t{s}\t{}\t{}", r[0], r[1]);
    }
    for (c, k) in &slots {
        let _ = writeln!(f, "slot_target\t{c}\t{k}");
    }
    for ((t, init), k) in &handle_holders {
        let _ = writeln!(f, "handle_holder\t{}\t{init}\t{k}", name(*t));
    }
    for ((t, is_live, init, ht, hb), k) in &special {
        let _ = writeln!(
            f,
            "special\t{t}\t{}\t{}\t{init}\t{}\t{hb}\t{k}",
            name(*t),
            if *is_live { "live" } else { "garbage" },
            if *ht == 0 { "-" } else { name(*ht) },
        );
    }
    for (fm, r) in &by_form {
        let _ = writeln!(f, "#live_form\t{fm}\t{}\t{}", r[0], r[1]);
    }
    let _ = writeln!(f, "#live\tn={live_n}\tbytes={live_b}");
    let _ = writeln!(f, "#garbage\tn={garb_n}\tbytes={garb_b}");
}
