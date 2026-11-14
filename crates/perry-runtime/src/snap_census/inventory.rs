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
//! - `#total ...` lines with the sums.

use std::collections::{BTreeMap, HashSet};
use std::io::Write;
use std::sync::atomic::{AtomicPtr, Ordering::Relaxed};

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

fn copy(lo: usize, hi: usize) -> (usize, Vec<u8>) {
    // SAFETY: `[lo, hi)` is mapped writable data of this process.
    (lo, unsafe { std::slice::from_raw_parts(lo as *const u8, hi - lo) }.to_vec())
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
        GC_TYPE_NATIVE_ARENA_OWNER | GC_TYPE_NATIVE_TYPED_VIEW | GC_TYPE_NATIVE_HANDLE
        | GC_TYPE_NATIVE_POD_VIEW | GC_TYPE_TEMPORAL | GC_TYPE_REGEX_PROGRAM
        | GC_TYPE_REGEX_CACHE | GC_TYPE_INTERP_FUNCTION | GC_TYPE_BUFFER_SECRET_KEY
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
    let a = if (v >> 48) >= 0x7ff8 { v & 0xffff_ffff_ffff } else { v } as usize;
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
        if holds { "collected" } else { "not-collected(no-hold)" }
    );
    // Cells: the arena walk and the malloc-tracked objects, minus free slots.
    let free: HashSet<usize> = crate::gc::ARENA_FREE_LIST
        .with(|fl| fl.borrow().iter().map(|&(ptr, _)| ptr as usize).collect());
    let mut headers: Vec<*mut u8> = Vec::new();
    crate::arena::arena_walk_objects(|h| headers.push(h));
    let malloc: Vec<*mut crate::gc::GcHeader> =
        crate::gc::MALLOC_STATE.with(|s| s.borrow().objects.clone());
    headers.extend(malloc.into_iter().map(|h| h as *mut u8));
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
        let r = rows.entry((t, init)).or_insert([0; 2]);
        r[0] += 1;
        r[1] += size as u64;
    }
    let mut by_type: BTreeMap<u8, [u64; 2]> = BTreeMap::new();
    for ((t, init), r) in &rows {
        let name = crate::gc::gc_type_info(*t).map_or("?", |i| i.name);
        let init = if *init < 0 { "-".to_owned() } else { init.to_string() };
        let _ = writeln!(f, "cell\t{t}\t{name}\t{}\t{init}\t{}\t{}", form(*t), r[0], r[1]);
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
