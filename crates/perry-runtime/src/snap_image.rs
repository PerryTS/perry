//! Image run: the runtime half of the execution-prefix image.
//!
//! A binary compiled with `PERRY_SNAP_IMAGE_RUN` calls [`js_snap_image_begin`]
//! first in `main` and [`js_snap_image_cut`] where the entry body starts, after
//! every static dependency has been initialised. With
//! `PERRY_SNAP_IMAGE_OUT=<file>` set, the cut marks the cells the collector's roots reach (read-only, no
//! collection), walks them, writes the image file (see `format`) and exits before the
//! entry body runs. Without it the cut returns at once. No other binary
//! reaches this code: the calls are emitted only for the knob.

pub mod format;
#[cfg(test)]
mod format_read;
#[cfg(test)]
mod tests;

use format::{Cell, Image, Stop, Val, Word};
use std::sync::atomic::{AtomicPtr, Ordering::Relaxed};

// ELF linker-defined symbols. Off Linux no image is captured (`begin` and
// `cut` are Linux x86_64 only), so `build` sees empty ranges there.
#[cfg(target_os = "linux")]
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

/// What `begin` saw: the writable data and the main thread's TLS block, and
/// the program's TLS slot ranges (data begin/end, bss begin/end).
pub(crate) struct Baseline {
    data: (usize, Vec<u8>),
    tls: (usize, Vec<u8>),
    /// The same ranges as taken first at the cut, before the capture ran any
    /// code of its own (filled in place: the copy allocates nothing). Empty =
    /// no cut copy: every change counts as the program's.
    pre_data: Vec<u8>,
    pre_tls: Vec<u8>,
    /// `class_image::snap_image_table_sizes` at the baseline.
    classes: [usize; format::CLASS_TABLE_COUNT],
    /// Hot-slot claims at the baseline.
    claims_from: u32,
    /// Names claimed since, read first at the cut (the capture claims its own).
    claims: Vec<(Vec<u8>, u32)>,
    /// Shape records and counters, and the next class-template token, at the baseline.
    shapes: crate::object::shapes::SnapShapeState,
    token: u64,
    /// The canonical trie at the baseline.
    canon: crate::object::canonical_keys::snap_image::CanonBaseline,
}

static BASELINE: AtomicPtr<Baseline> = AtomicPtr::new(std::ptr::null_mut());

fn copy(lo: usize, hi: usize) -> (usize, Vec<u8>) {
    // SAFETY: `[lo, hi)` is mapped writable memory of this process.
    (
        lo,
        unsafe { std::slice::from_raw_parts(lo as *const u8, hi - lo) }.to_vec(),
    )
}

/// Size of the executable's static TLS block (x86-64 variant II: the block
/// ends at the thread pointer). Only the Linux x86-64 capture calls it, and the
/// `dl_iterate_phdr` family it uses is missing from `libc` on tvOS and watchOS.
#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
fn executable_tls_size() -> (usize, usize) {
    unsafe extern "C" fn each(
        info: *mut libc::dl_phdr_info,
        _size: libc::size_t,
        data: *mut libc::c_void,
    ) -> libc::c_int {
        let info = unsafe { &*info };
        for i in 0..info.dlpi_phnum as usize {
            let ph = unsafe { &*info.dlpi_phdr.add(i) };
            if ph.p_type == libc::PT_TLS {
                let align = (ph.p_align as usize).max(1);
                unsafe { *(data as *mut (usize, usize)) = (ph.p_memsz as usize, align) };
            }
        }
        1
    }
    let mut size = (0usize, 1usize);
    unsafe { libc::dl_iterate_phdr(Some(each), (&mut size as *mut (usize, usize)).cast()) };
    size
}

#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
#[no_mangle]
pub extern "C" fn js_snap_image_begin(
    _tls_data_lo: usize,
    _tls_data_hi: usize,
    _tls_bss_lo: usize,
    _tls_bss_hi: usize,
) {
    let mut fs = 0usize;
    // SAFETY: ARCH_GET_FS stores the thread pointer.
    unsafe { libc::syscall(libc::SYS_arch_prctl, 0x1003, &mut fs as *mut usize) };
    let (memsz, align) = executable_tls_size();
    let (tls_lo, tls_len) = tls_window(fs, memsz, align);
    let lo = std::ptr::addr_of!(__data_start) as usize;
    let hi = std::ptr::addr_of!(_end) as usize;
    let pre_data = vec![0u8; hi - lo];
    let pre_tls = vec![0u8; tls_len];
    let b = Box::new(Baseline {
        data: copy(lo, hi),
        tls: copy(tls_lo, tls_lo + tls_len),
        pre_data,
        pre_tls,
        classes: crate::object::class_image::snap_image_table_sizes(),
        claims_from: crate::tls_hot::js_tls_hot_claimed_slots(),
        claims: Vec::new(),
        shapes: crate::object::shapes::snap_image_shape_state(),
        token: crate::object::field_get_set::snap_image_next_token(),
        canon: crate::object::canonical_keys::snap_image::snap_image_canon_baseline(),
    });
    BASELINE.store(Box::into_raw(b), Relaxed);
}

#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
#[unsafe(naked)]
#[no_mangle]
pub extern "C" fn js_snap_image_cut() {
    crate::gc::offering_entry_asm!(0, js_snap_image_cut_from)
}

/// The cut entry offers its caller's frame to the root walk, as `gc()` does.
#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
extern "C" fn js_snap_image_cut_from(pc: usize, sp: usize, fp: usize) {
    // First, before the offer below writes the walk origin and before any
    // other code of the capture: the words as the program left them.
    let p = BASELINE.load(Relaxed);
    if !p.is_null() {
        // SAFETY: leaked in `begin`, never freed; one cut per process.
        take_pre(unsafe { &mut *p });
    }
    let origin = crate::gc::WalkOrigin { pc, sp, fp };
    let _origin = crate::gc::EntryWalkOrigin::enter(&origin);
    cut_body();
}

#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
fn cut_body() {
    let p = BASELINE.load(Relaxed);
    if p.is_null() {
        return;
    }
    // SAFETY: leaked in `begin`, never freed; one cut per process.
    let b = unsafe { &mut *p };
    let Some(out) = std::env::var_os("PERRY_SNAP_IMAGE_OUT") else {
        return;
    };
    BASELINE.store(std::ptr::null_mut(), Relaxed);
    let b = &*b;
    let pre = read_maps();
    let img = build(Some(b), pre, true);
    let ok = std::fs::write(&out, format::encode(&img)).is_ok();
    std::process::exit(if ok { 0 } else { 70 });
}

/// Copy the data range and the TLS block into the buffers `begin` made.
fn take_pre(b: &mut Baseline) {
    b.claims = crate::tls_hot::claims_in_order(b.claims_from);
    for (lo, buf) in [(b.data.0, &mut b.pre_data), (b.tls.0, &mut b.pre_tls)] {
        // SAFETY: the range `begin` copied; the buffer has the same length.
        unsafe { std::ptr::copy_nonoverlapping(lo as *const u8, buf.as_mut_ptr(), buf.len()) };
    }
}

/// A GC type with no image form: a native payload (inventory `form`).
fn no_form(t: u8) -> bool {
    use crate::gc::*;
    matches!(
        t,
        GC_TYPE_NATIVE_ARENA_OWNER
            | GC_TYPE_NATIVE_TYPED_VIEW
            | GC_TYPE_NATIVE_HANDLE
            | GC_TYPE_NATIVE_POD_VIEW
            | GC_TYPE_TEMPORAL
            | GC_TYPE_REGEX_CACHE
            | GC_TYPE_INTERP_FUNCTION
            | GC_TYPE_BUFFER_SECRET_KEY
            | GC_TYPE_BUFFER_CRYPTO_KEY
            | GC_TYPE_PROMISE
            | GC_TYPE_REGEX_PROGRAM
    ) || crate::gc::gc_type_info(t).is_none()
}

/// Where an address lies, for classifying a word.
struct Where {
    /// (user address, header address, size), sorted by address.
    cells: Vec<(usize, usize, usize)>,
    sections: [(usize, usize); 3],
    exe: (usize, usize),
    tls: (usize, usize),
    /// Sorted, merged address ranges of the process (`/proc/self/maps`).
    maps: Vec<(usize, usize)>,
    /// The persistent-symbol space.
    sym: (usize, usize),
}

impl Where {
    fn val(&self, bits: u64) -> Result<Val, u64> {
        use crate::value::{BIGINT_TAG, POINTER_MASK, POINTER_TAG, STRING_TAG, TAG_MASK};
        let tag = bits & TAG_MASK;
        // Strip the NaN-box tag: the payload is what could be an address.
        let (hi, bare, a) = if tag == POINTER_TAG || tag == STRING_TAG || tag == BIGINT_TAG {
            ((bits >> 48) as u16, false, (bits & POINTER_MASK) as usize)
        } else {
            (0, true, bits as usize)
        };
        // A payload outside every mapping cannot be dereferenced: a scalar.
        if !self.mapped(a) {
            return Ok(Val::Scalar(bits));
        }
        let i = self.cells.partition_point(|c| c.1 <= a);
        if i > 0 {
            let (user, header, size) = self.cells.get(i - 1).copied().unwrap_or((0, 0, 0));
            if a < header + size {
                return if a == user {
                    Ok(Val::Cell {
                        hi,
                        bare,
                        index: (i - 1) as u32,
                    })
                } else {
                    Err(bits)
                };
            }
        }
        let within = |r: (usize, usize)| a >= r.0 && a < r.1;
        let regions = [
            format::REGION_STRINGS,
            format::REGION_LAYOUTS,
            format::REGION_ATOMS,
        ];
        for (&region, s) in regions.iter().zip(&self.sections) {
            if within(*s) {
                return Ok(Val::Ext {
                    hi,
                    bare,
                    region,
                    off: (a - s.0) as u64,
                });
            }
        }
        if within(self.tls) {
            return Ok(Val::Ext {
                hi,
                bare,
                region: format::REGION_TLS,
                off: (a - self.tls.0) as u64,
            });
        }
        if within(self.exe) {
            return Ok(Val::Ext {
                hi,
                bare,
                region: format::REGION_EXE,
                off: (a - self.exe.0) as u64,
            });
        }
        if a >= self.sym.0 && a < self.sym.1 {
            return Ok(Val::Sym(bits));
        }
        Ok(Val::Opaque(bits))
    }

    fn mapped(&self, a: usize) -> bool {
        let i = self.maps.partition_point(|r| r.0 <= a);
        i.checked_sub(1)
            .and_then(|j| self.maps.get(j))
            .is_some_and(|r| a < r.1)
    }
}

/// Header addresses of every cell the allocators hold.
fn cell_headers() -> Vec<usize> {
    use crate::gc::*;
    let free: std::collections::HashSet<usize> = ARENA_FREE_LIST
        .try_with(|fl| {
            fl.try_borrow()
                .map(|v| v.iter().map(|&(ptr, _)| ptr as usize).collect())
                .unwrap_or_default()
        })
        .unwrap_or_default();
    let mut headers: Vec<usize> = Vec::new();
    crate::arena::arena_walk_objects(|h| headers.push(h as usize));
    let malloc: Vec<*mut GcHeader> = MALLOC_STATE
        .try_with(|s| {
            s.try_borrow()
                .map(|v| v.objects.clone())
                .unwrap_or_default()
        })
        .unwrap_or_default();
    headers.extend(malloc.into_iter().map(|h| h as usize));
    headers.retain(|&a| {
        // SAFETY: headers of cells the allocators hold.
        let g = unsafe { &*(a as *const GcHeader) };
        g.obj_type != 0
            && g.obj_type <= GC_TYPE_MAX
            && g.size != 0
            && g.gc_flags & GC_FLAG_FORWARDED == 0
            && !free.contains(&a)
            && !free.contains(&(a + GC_HEADER_SIZE))
    });
    headers.sort_unstable();
    headers.dedup();
    headers
}

/// The image of the heap now and, with a baseline, of the root words that
/// changed since `begin`.
pub(crate) fn build(base: Option<&Baseline>, pre: Vec<(usize, usize)>, live: bool) -> Image {
    use crate::gc::{GcHeader, GC_HEADER_SIZE};
    let mut headers = cell_headers();
    // The shape records minted since the baseline name their key lists and prototypes, and
    // the trie names the backings of its new and changed nodes: the state at the cut holds
    // them even where no live object does, so they are roots of the mark.
    let mut mint = base.map(|b| crate::object::shapes::snap_image_mint_collect(&b.shapes));
    if live {
        headers = mark_live(headers, |v| {
            crate::gc::image_visit_roots(v);
            if let (Some(b), Some(m)) = (base, &mint) {
                mint_roots(m, v);
                crate::object::canonical_keys::snap_image::snap_image_canon_words(&b.canon, v);
            }
        });
    }
    let mut maps = pre;
    maps.extend(read_maps());
    #[cfg(target_os = "linux")]
    let sect = |lo: &u8, hi: &u8| (lo as *const u8 as usize, hi as *const u8 as usize);
    // SAFETY: linker-defined section bounds.
    let w = unsafe {
        Where {
            cells: headers
                .iter()
                .map(|&h| {
                    (
                        h + GC_HEADER_SIZE,
                        h,
                        (*(h as *const GcHeader)).size as usize,
                    )
                })
                .collect(),
            #[cfg(target_os = "linux")]
            sections: [
                sect(&__start_perry_strings, &__stop_perry_strings),
                sect(&__start_perry_layouts, &__stop_perry_layouts),
                sect(&__start_perry_atoms, &__stop_perry_atoms),
            ],
            #[cfg(not(target_os = "linux"))]
            sections: [(0, 0); 3],
            #[cfg(target_os = "linux")]
            exe: sect(&__executable_start, &_end),
            #[cfg(not(target_os = "linux"))]
            exe: (0, 0),
            tls: base.map_or((0, 0), |b| (b.tls.0, b.tls.0 + b.tls.1.len())),
            maps: merged(maps),
            sym: crate::symbol::snap_image_space(),
        }
    };
    let mut img = Image::default();
    for (i, &h) in headers.iter().enumerate() {
        let hp = h as *mut GcHeader;
        // SAFETY: a cell the allocators hold; `size` is its total size.
        let (t, flags, reserved, size) = unsafe {
            let g = &*hp;
            (g.obj_type, g.gc_flags, g._reserved, g.size)
        };
        let mut bytes =
            unsafe { std::slice::from_raw_parts(h as *const u8, size as usize) }.to_vec();
        if no_form(t) {
            img.stops.push(Stop {
                cell: i as u32,
                obj_type: t,
                kind: format::STOP_NO_FORM,
                off: 0,
                value: 0,
            });
        }
        let mut slots: Vec<(u32, Val)> = Vec::new();
        // SAFETY: visits the traced slots of a live cell.
        unsafe {
            crate::gc::image_cells::visit_cell_slots_raw(hp, &mut |slot, bits, own| {
                // A shape record word (keys, prototype) lives in the shape
                // table: its form is the shape slab (a `format::PENDING`
                // bit), not this cell.
                if !own {
                    return;
                }
                let off = slot.wrapping_sub(h);
                let Some(word) = off.checked_add(8).and_then(|end| bytes.get_mut(off..end)) else {
                    img.stops.push(Stop {
                        cell: i as u32,
                        obj_type: t,
                        kind: format::STOP_SLOT_OUTSIDE,
                        off: off as u32,
                        value: 0,
                    });
                    return;
                };
                word.fill(0);
                match w.val(bits) {
                    Ok(v) => slots.push((off as u32, v)),
                    Err(b) => {
                        img.stops.push(Stop {
                            cell: i as u32,
                            obj_type: t,
                            kind: format::STOP_NOT_CELL_START,
                            off: off as u32,
                            value: b,
                        });
                        slots.push((off as u32, Val::Opaque(b)));
                    }
                }
            });
        }
        cell_forms(
            &w,
            &headers,
            i,
            h,
            t,
            &mut bytes,
            &mut slots,
            &mut img.stops,
        );
        slots.sort_by_key(|s| s.0);
        img.cells.push(Cell {
            obj_type: t,
            flags,
            reserved,
            size,
            bytes,
            slots,
        });
    }
    if let Some(b) = base {
        let own = std::ptr::addr_of!(BASELINE) as usize;
        let le = |s: &[u8]| u64::from_le_bytes(s.try_into().unwrap_or([0; 8]));
        let regions = [
            (format::WORD_DATA, &b.data, &b.pre_data),
            (format::WORD_TLS, &b.tls, &b.pre_tls),
        ];
        for (region, (lo, old), pre) in regions {
            for (i, o) in old.chunks_exact(8).enumerate() {
                let a = lo + i * 8;
                if a >= own && a < own + 8 {
                    continue;
                }
                // The word as the program left it at the cut: the copy taken
                // first, never the live word (the capture's own code runs
                // after that copy).
                let now = pre.get(i * 8..i * 8 + 8).map_or(0, le);
                let was = le(o);
                if now == was {
                    continue;
                }
                let val = w.val(now).unwrap_or(Val::Opaque(now));
                img.words.push(Word {
                    region,
                    off: (a - lo) as u64,
                    val,
                });
            }
        }
    }
    if let Some(b) = base {
        img.claims_from = b.claims_from;
        for (k, i) in &b.claims {
            if k.len() > 4096 || std::str::from_utf8(k).is_err() && !k.starts_with(b"anonymous:") {
                img.stops.push(Stop {
                    cell: *i,
                    obj_type: 0,
                    kind: format::STOP_CLAIM_KEY,
                    off: 0,
                    value: 0,
                });
            }
            img.claims.push(k.clone());
        }
        let now = crate::object::class_image::snap_image_table_sizes();
        for (i, (n, was)) in now.iter().zip(b.classes.iter()).enumerate() {
            if n != was {
                img.stops.push(Stop {
                    cell: i as u32,
                    obj_type: 0,
                    kind: format::STOP_CLASS_TABLES,
                    off: 0,
                    value: n.wrapping_sub(*was) as u64,
                });
            }
        }
    }
    match base {
        Some(b) => {
            let delta = crate::object::shapes::snap_image_shape_delta(
                &b.shapes,
                &crate::object::shapes::snap_image_shape_state(),
            );
            img.mint = mint.take().unwrap_or_default();
            let mint_notes = mint_capture(
                &w,
                &mut img.mint,
                b.data.0,
                &b.data.1,
                &b.pre_data,
                &b.shapes,
            );
            // A torn-down trie leaves the record out: the replay needs it, so the image stops.
            // The addresses the record names, with their image form, for the replay check.
            let mut named: Vec<(Val, usize)> = Vec::new();
            let canon = crate::object::canonical_keys::snap_image::snap_image_canon_capture(
                &b.canon,
                &mut |a| {
                    let v = w.val(a as u64).unwrap_or_else(Val::Opaque);
                    named.push((v.clone(), a));
                    v
                },
            );
            let mint_stop = mint_stop(&delta, &img.mint);
            let (pending, stops) = completeness(
                delta,
                b.token,
                crate::object::field_get_set::snap_image_next_token(),
            );
            img.pending = pending;
            img.stops.extend(stops);
            img.stops.extend(mint_stop);
            // A record keying a canonical list needs the canonical-keys record, which replays
            // its node: the stop stands only where the record is left out.
            let held = canon.is_some();
            img.stops.extend(
                mint_notes
                    .into_iter()
                    .filter(|s| !(held && s.kind == format::STOP_MINT_CANONICAL)),
            );
            if let Some((canon, canon_stops)) = canon {
                img.stops.extend(canon_stops);
                img.stops.push(canon_stop(&canon));
                img.stops.push(canon_replay_stop(&b.canon, &canon, named));
                img.canon = Some(canon);
            }
            img.registry = registry_record(&w, &mut img.stops);
        }
        None => img.pending = format::PENDING,
    }
    img
}

/// The address ranges of `/proc/self/maps` now.
fn read_maps() -> Vec<(usize, usize)> {
    // A plain path, not OsStr::from_bytes: that is Unix-only and broke the Windows build.
    // Where the file does not exist (macOS, Windows) the result is empty, as before.
    let text = std::fs::read("/proc/self/maps").unwrap_or_default();
    parse_maps(&text)
}

fn parse_maps(text: &[u8]) -> Vec<(usize, usize)> {
    let hex = |s: &[u8]| {
        s.iter().try_fold(0usize, |a, &c| {
            (c as char)
                .to_digit(16)
                .map(|d| a.wrapping_mul(16).wrapping_add(d as usize))
        })
    };
    let mut v = Vec::new();
    for line in text.split(|&c| c == b'\n') {
        let range = line.split(|&c| c == b' ').next().unwrap_or_default();
        let mut it = range.split(|&c| c == b'-');
        if let (Some(lo), Some(hi)) = (it.next().and_then(hex), it.next().and_then(hex)) {
            if lo < hi {
                v.push((lo, hi));
            }
        }
    }
    v
}

/// Sorted ranges with overlapping or touching ones joined.
fn merged(mut v: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    v.sort_unstable();
    let mut out: Vec<(usize, usize)> = Vec::new();
    for r in v {
        match out.last_mut() {
            Some(l) if r.0 <= l.1 => l.1 = l.1.max(r.1),
            _ => out.push(r),
        }
    }
    out
}

/// The headers among `headers` (sorted) that `roots` reach through the traced
/// slots: a read-only mark into a local flag per header, dropped on return.
/// Nothing is swept, moved, promoted or counted.
fn mark_live(headers: Vec<usize>, roots: impl FnOnce(&mut dyn FnMut(u64))) -> Vec<usize> {
    use crate::gc::{GcHeader, GC_HEADER_SIZE};
    use crate::value::{BIGINT_TAG, POINTER_MASK, POINTER_TAG, STRING_TAG, TAG_MASK};
    let cell_of = |hs: &[usize], bits: u64| -> Option<usize> {
        let tag = bits & TAG_MASK;
        let a = if tag == POINTER_TAG || tag == STRING_TAG || tag == BIGINT_TAG {
            (bits & POINTER_MASK) as usize
        } else {
            bits as usize
        };
        hs.binary_search(&a.wrapping_sub(GC_HEADER_SIZE)).ok()
    };
    let mut marked = vec![false; headers.len()];
    let mut work: Vec<usize> = Vec::new();
    let mut found: Vec<u64> = Vec::new();
    roots(&mut |bits| found.push(bits));
    loop {
        for bits in found.drain(..) {
            if let Some((i, m)) =
                cell_of(&headers, bits).and_then(|i| Some((i, marked.get_mut(i)?)))
            {
                if !*m {
                    *m = true;
                    work.push(i);
                }
            }
        }
        let Some(&h) = work.pop().and_then(|i| headers.get(i)) else {
            break;
        };
        // SAFETY: a cell the allocators hold; the slots are read in place.
        unsafe {
            crate::gc::image_cells::visit_cell_slots_raw(
                h as *mut GcHeader,
                &mut |_slot, bits, _own| found.push(bits),
            );
        }
    }
    headers
        .into_iter()
        .zip(marked)
        .filter_map(|(h, m)| m.then_some(h))
        .collect()
}

/// The words a mint record references: its key list and the content of its prototype slot.
fn mint_roots(recs: &[format::MintRec], v: &mut dyn FnMut(u64)) {
    for r in recs {
        if r.keys != 0 {
            v(r.keys);
        }
        if r.proto_cell != 0 {
            // SAFETY: the slot of the prototype identity words, a live allocation of the shape table.
            v(unsafe { std::ptr::read_volatile(r.proto_cell as usize as *const u64) });
        }
    }
}

/// The canonical-keys record is written and read back: the stop that reports its size and
/// node counts.
fn canon_stop(c: &format::CanonRec) -> Stop {
    let blob = format::canon_blob(c);
    let same = format::parse_canon_blob(&blob)
        .is_some_and(|(back, used)| used == blob.len() && back == *c);
    Stop {
        cell: u32::from(!same),
        obj_type: 0,
        kind: format::STOP_CANON_RECORD,
        off: blob.len() as u32,
        value: c.new.len() as u64 | (c.changed.len() as u64) << 32,
    }
}

/// The canonical-keys record as the file holds it, replayed into a trie rebuilt from the
/// baseline and compared with the trie at the cut: the stop that reports the outcome.
fn canon_replay_stop(
    b: &crate::object::canonical_keys::snap_image::CanonBaseline,
    c: &format::CanonRec,
    named: Vec<(Val, usize)>,
) -> Stop {
    use crate::object::canonical_keys::snap_image as ck;
    // The replay overwrites its thread's trie: it runs on a thread of its own, which owns a
    // copy of the baseline, the record and the address list. The builder's spawn carries no
    // panic site (a scope would bring one, with this file's location string, into every
    // binary that links the image module); a failed spawn or a panicked replay is an error.
    let check = format::parse_canon_blob(&format::canon_blob(c)).and_then(|(back, _)| {
        let base = b.copy();
        let worker = std::thread::Builder::new()
            .spawn(move || {
                let addr = |v: &Val| match v {
                    Val::Scalar(x) => *x as usize,
                    _ => named.iter().find(|n| n.0 == *v).map_or(0, |n| n.1),
                };
                ck::snap_image_canon_replay_apart(&base, &back, &addr)
            })
            .ok()?;
        let replayed = worker.join().ok().flatten()?;
        ck::snap_image_canon_check(replayed)
    });
    let first = match check {
        Some(Ok(())) => None,
        Some(Err(e)) => Some(e.node),
        None => Some(u32::MAX),
    };
    Stop {
        cell: u32::from(first.is_some()),
        obj_type: 0,
        kind: format::STOP_CANON_REPLAY,
        off: first.unwrap_or(0),
        value: u64::from(c.cut_len),
    }
}

/// The mint record of the shape records minted since the baseline is written (as the file
/// holds it) and a table rebuilt from it is compared with the live one: the stop that
/// reports its size, or the first record whose rebuilt form differs.
fn mint_stop(
    delta: &crate::object::shapes::SnapShapeDelta,
    recs: &[format::MintRec],
) -> Option<Stop> {
    if recs.is_empty() && crate::object::shapes::snap_image_delta_minted_total(delta) == 0 {
        return None;
    }
    let blob = format::mint_blob(recs);
    let back = format::parse_mint_blob(&blob)
        .map(|b| b.0)
        .unwrap_or_default();
    let bad = if back.len() == recs.len() {
        // The references are added after the live record is read: the check compares the rest.
        let mut plain = Vec::new();
        for r in &back {
            plain.push(format::mint_without_refs(r));
        }
        crate::object::shapes::snap_image_mint_check(&plain)
    } else {
        Some((back.len().min(recs.len()), format::MINT_BAD_FIELDS))
    };
    let mut stop = Stop {
        cell: 0,
        obj_type: 0,
        kind: format::STOP_MINT_RECORD,
        off: 0,
        value: 0,
    };
    if let Some((i, why)) = bad {
        stop.kind = format::STOP_MINT_MISMATCH;
        stop.cell = i as u32;
        stop.off = why;
        stop.value = u64::from(recs.get(i).map_or(0, |r| r.id));
        return Some(stop);
    }
    // Records per id band, as the delta counts them (16 bits each, saturating).
    let sat = |n: u32| n.min(0xffff);
    let m = delta.minted;
    stop.cell = sat(m[0]) | sat(m[2]) << 16;
    stop.off = sat(m[1]) | sat(m[3]) << 16;
    stop.value = recs.len() as u64 | (blob.len() as u64) << 32;
    Some(stop)
}

/// A reference to a word that is valid in the relinked binary: an image cell or binary data. A scalar
/// is accepted when it is zero, or when `scalar_any` (a word that is not an address, such as a tagged
/// null prototype); a heap or stack address that is not a cell is not a reference.
fn mint_ref(w: &Where, bits: u64, scalar_any: bool) -> Option<Val> {
    match w.val(bits) {
        Ok(Val::Scalar(x)) if scalar_any || x == 0 => Some(Val::Scalar(x)),
        Ok(v @ (Val::Cell { .. } | Val::Ext { .. })) => Some(v),
        _ => None,
    }
}

/// The runtime layout descriptors whose `shape_id` word changed to `id` between the baseline copy `was`
/// and the cut copy `now` of the data window, and whose `layout_id` is `word`: how many, and the offset
/// of the last from the window start.
fn mint_desc_matches(was: &[u8], now: &[u8], id: u32, word: u32) -> (u32, u64) {
    use crate::object::layout_desc::PerryLayoutDesc;
    let (so, lo) = (
        std::mem::offset_of!(PerryLayoutDesc, shape_id),
        std::mem::offset_of!(PerryLayoutDesc, layout_id),
    );
    let u32at = |b: &[u8], o: usize| {
        b.get(o..o + 4)
            .map_or(0, |s| u32::from_le_bytes(s.try_into().unwrap_or([0; 4])))
    };
    let (mut n, mut at) = (0u32, 0u64);
    let words = now.len().min(was.len()) / 8;
    for i in 0..words {
        if now.get(i * 8..i * 8 + 8) == was.get(i * 8..i * 8 + 8) {
            continue;
        }
        for half in 0..2usize {
            let c = i * 8 + half * 4;
            if u32at(now, c) != id {
                continue;
            }
            let Some(d) = c.checked_sub(so) else { continue };
            if u32at(now, d + lo) == word {
                n += 1;
                at = d as u64;
            }
        }
    }
    (n, at)
}

/// The prototype serial the first birth of the descriptor at `desc_off` drew for its family: the
/// family's serial word changed from 0 between the baseline copy `was` and the cut copy `now` of
/// the data window (which starts at `data_lo`), and no record earlier in mint order drew it
/// (`families`). 0 when the birth drew none.
fn mint_fam_serial(
    was: &[u8],
    now: &[u8],
    data_lo: usize,
    desc_off: u64,
    families: &mut Vec<u64>,
) -> u64 {
    use crate::object::layout_desc as ld;
    let u64at = |b: &[u8], o: u64| {
        usize::try_from(o)
            .ok()
            .and_then(|o| b.get(o..o.checked_add(8)?))
            .map_or(0, |s| u64::from_le_bytes(s.try_into().unwrap_or([0; 8])))
    };
    let proto = std::mem::offset_of!(ld::PerryLayoutDesc, proto) as u64;
    let fam = u64at(now, desc_off.saturating_add(proto));
    if fam < ld::LAYOUT_PROTO_TAG_END as u64 {
        return 0;
    }
    let Some(off) = fam.checked_sub(data_lo as u64) else {
        return 0;
    };
    let off = off.saturating_add(ld::SNAP_IMAGE_FAMILY_SERIAL_OFFSET as u64);
    let (old, new) = (u64at(was, off), u64at(now, off));
    if old != 0 || new == 0 || families.contains(&off) {
        return 0;
    }
    families.push(off);
    new
}

fn mint_bump(c: &mut [u64], k: usize) {
    if let Some(x) = c.get_mut(k) {
        *x += 1;
    }
}

fn mint_set(c: &mut [u64], k: usize, v: u64) {
    if let Some(x) = c.get_mut(k) {
        *x = v;
    }
}

fn mint_note(n: &mut [u32], first: &mut [usize], k: usize, i: usize) {
    if let (Some(c), Some(f)) = (n.get_mut(k), first.get_mut(k)) {
        if *c == 0 {
            *f = i;
        }
        *c += 1;
    }
}

/// What replay needs of the minted records and the capture adds: each record's references (key list,
/// prototype word, a runtime layout's descriptor cell), a stop per reason replay cannot rebuild a record
/// through the normal mint path (the first record and how many), and the census stops. `was` and `now`
/// are the data window at the baseline and at the cut.
fn mint_capture(
    w: &Where,
    recs: &mut [format::MintRec],
    data_lo: usize,
    was: &[u8],
    now: &[u8],
    begin: &crate::object::shapes::SnapShapeState,
) -> Vec<Stop> {
    use crate::object::shapes as sh;
    use format as f;
    let mut stops = Vec::new();
    if recs.is_empty() {
        return stops;
    }
    let flags = sh::snap_image_mint_replay_flags(recs);
    // Reasons: keys, canonical, proto word, no descriptor, row gap, successor.
    let (mut n, mut first): (Vec<u32>, Vec<usize>) = (Vec::new(), Vec::new());
    // Census items (`MINT_CENSUS_*` order).
    let mut c: Vec<u64> = Vec::new();
    // The family serial words a record earlier in mint order drew (offsets in the window).
    let mut families: Vec<u64> = Vec::new();
    for _ in 0..6 {
        n.push(0);
        first.push(0);
    }
    for _ in 0..f::MINT_CENSUS_ITEMS {
        c.push(0);
    }
    for (i, r) in recs.iter_mut().enumerate() {
        if let Some(x) = c.get_mut(r.route as usize) {
            *x += 1;
        }
        r.keys_ref = mint_ref(w, r.keys, false);
        match &r.keys_ref {
            Some(Val::Cell { .. }) => mint_bump(&mut c, 14),
            Some(Val::Ext { .. }) => mint_bump(&mut c, 15),
            Some(_) => {}
            None => mint_note(&mut n, &mut first, 0, i),
        }
        if r.proto_cell != 0 {
            // SAFETY: the slot of the prototype identity words, a live allocation of the shape table.
            let bits = unsafe { std::ptr::read_volatile(r.proto_cell as usize as *const u64) };
            r.proto_ref = mint_ref(w, bits, true);
            if r.proto_ref.is_none() {
                mint_note(&mut n, &mut first, 2, i);
            }
        }
        if r.route == f::MINT_ROUTE_RUNTIME {
            let (m, off) = mint_desc_matches(was, now, r.id, r.word);
            if m == 1 {
                r.desc_off = Some(off);
                mint_bump(&mut c, 16);
                r.fam_serial = mint_fam_serial(was, now, data_lo, off, &mut families);
                if r.fam_serial != 0 {
                    mint_bump(&mut c, 17);
                }
            } else {
                mint_note(&mut n, &mut first, 3, i);
            }
        }
        if r.route == f::MINT_ROUTE_LAZY {
            let d = crate::object::layout_desc::snap_image_lazy_descriptor(r.word);
            if let Some(off) = d.checked_sub(data_lo).filter(|_| d != 0) {
                r.fam_serial = mint_fam_serial(was, now, data_lo, off as u64, &mut families);
                if r.fam_serial != 0 {
                    mint_bump(&mut c, 17);
                }
            }
        }
        let fl = flags.get(i).copied().unwrap_or(0);
        if fl & f::MINT_FLAG_CANONICAL != 0 {
            mint_bump(&mut c, 4);
            mint_note(&mut n, &mut first, 1, i);
        }
        if fl & f::MINT_FLAG_SUCCESSOR != 0 {
            mint_bump(&mut c, 5);
            mint_note(&mut n, &mut first, 5, i);
        }
        if fl & f::MINT_FLAG_ROW_GAP != 0 {
            mint_bump(&mut c, 6);
            mint_note(&mut n, &mut first, 4, i);
        }
        let p = r.proto_id;
        let k = if p == crate::object::shapes::PROTO_ID_DEFAULT {
            7
        } else if (sh::PROTO_ID_CLASS..sh::PROTO_ID_MIXED).contains(&p) {
            8
        } else if (sh::PROTO_ID_MIXED..sh::PROTO_ID_UNIQUE).contains(&p) {
            9
        } else if (sh::PROTO_ID_UNIQUE..sh::PROTO_ID_RECORD_HELD).contains(&p) {
            10
        } else {
            11
        };
        mint_bump(&mut c, k);
    }
    let (unique, serial) = sh::snap_image_mint_counters_moved(begin);
    mint_set(&mut c, 12, unique);
    mint_set(&mut c, 13, serial);
    let kinds = [
        f::STOP_MINT_KEYS,
        f::STOP_MINT_CANONICAL,
        f::STOP_MINT_PROTO_WORD,
        f::STOP_MINT_NO_DESC,
        f::STOP_MINT_ROW_GAP,
        f::STOP_MINT_SUCCESSOR,
    ];
    for (kind, (cnt, at)) in kinds.iter().zip(n.iter().zip(first.iter())) {
        if *cnt > 0 {
            let id = recs.get(*at).map_or(0, |r| r.id);
            stops.push(Stop {
                cell: *at as u32,
                obj_type: 0,
                kind: *kind,
                off: *cnt,
                value: u64::from(id),
            });
        }
    }
    for (item, value) in c.iter().enumerate() {
        stops.push(Stop {
            cell: 0,
            obj_type: item as u8,
            kind: f::STOP_MINT_CENSUS,
            off: 0,
            value: *value,
        });
    }
    stops
}

/// Which forms the image still lacks, and why it stops: a shape minted between the
/// baseline and the cut (the shape slab has no form), more than the primary's
/// class-template token. The atom table has no run-time counter: its bit stays set.
fn completeness(
    shapes: crate::object::shapes::SnapShapeDelta,
    tok0: u64,
    tok1: u64,
) -> (u32, Vec<Stop>) {
    let mut pending = format::PENDING_ATOMS;
    let mut stops = Vec::new();
    let mut stop = |kind: u8, cell: u32, off: u32, value: u64| {
        stops.push(Stop {
            cell,
            obj_type: 0,
            kind,
            off,
            value,
        });
    };
    if crate::object::shapes::snap_image_delta_any(&shapes) {
        pending |= format::PENDING_SHAPES;
    }
    if shapes.changed != 0 || shapes.retired != 0 {
        // One stop: the records minted per id band (16 bits each, saturating), the
        // pre-existing records that changed (`off`) and those that are gone (`cell`).
        let sat = |n: u32| u64::from(n.min(0xffff));
        let m = shapes.minted;
        stop(
            format::STOP_SHAPES_MINTED,
            shapes.retired,
            shapes.changed,
            sat(m[0]) | sat(m[1]) << 16 | sat(m[2]) << 32 | sat(m[3]) << 48,
        );
    }
    if shapes.drawn != 0 || shapes.generations != 0 {
        stop(
            format::STOP_SHAPE_COUNTERS,
            0,
            0,
            u64::from(shapes.drawn) | u64::from(shapes.generations) << 32,
        );
    }
    if tok1 > 2.max(tok0) {
        pending |= format::PENDING_TOKENS;
        stop(format::STOP_TOKENS, 0, 0, tok1);
    }
    (pending, stops)
}

/// The forms of one cell: the Map entries alias and a store's native key indexes are
/// derived (zeroed, named by a `Derived` slot); any other untraced word that holds an
/// address outside the binary's own sections stops the image.
#[allow(clippy::too_many_arguments)]
fn cell_forms(
    w: &Where,
    headers: &[usize],
    i: usize,
    h: usize,
    t: u8,
    bytes: &mut [u8],
    slots: &mut Vec<(u32, Val)>,
    stops: &mut Vec<Stop>,
) {
    use crate::gc::{GC_HEADER_SIZE, GC_TYPE_MAP, GC_TYPE_MAP_STORE};
    let le = |b: &[u8], o: usize| {
        b.get(o..o + 8)
            .map_or(0, |s| u64::from_le_bytes(s.try_into().unwrap_or([0; 8])))
    };
    let stop = |stops: &mut Vec<Stop>, kind: u8, off: usize, value: u64| {
        stops.push(Stop {
            cell: i as u32,
            obj_type: t,
            kind,
            off: off as u32,
            value,
        });
    };
    let f = crate::map::image_form();
    let mut derived: Vec<(usize, u8)> = Vec::new();
    let user = h + GC_HEADER_SIZE;
    if t == GC_TYPE_MAP {
        let (eo, so) = (
            GC_HEADER_SIZE + f.header_entries,
            GC_HEADER_SIZE + f.header_store,
        );
        let store_user = slots
            .iter()
            .find(|s| s.0 as usize == so)
            .and_then(|s| match s.1 {
                Val::Cell { index, .. } => headers.get(index as usize).map(|h| h + GC_HEADER_SIZE),
                _ => None,
            });
        let alias = le(bytes, eo);
        if store_user.is_some_and(|u| alias == (u + f.inline) as u64) {
            derived.push((eo, format::DERIVED_MAP_ENTRIES));
        } else {
            stop(stops, format::STOP_RAW_HEAP_WORD, eo, alias);
        }
    } else if t == GC_TYPE_MAP_STORE {
        // SAFETY: a store cell the allocators hold.
        if !unsafe { crate::map::image_keys_derivable(user as *const crate::map::MapStore) } {
            stop(stops, format::STOP_MAP_KEYS, 0, 0);
        }
        let eo = GC_HEADER_SIZE + f.entries;
        let alias = le(bytes, eo);
        if alias == (user + f.inline) as u64 {
            derived.push((eo, format::DERIVED_STORE_ENTRIES));
        } else {
            stop(stops, format::STOP_RAW_HEAP_WORD, eo, alias);
        }
        let mut o = GC_HEADER_SIZE + f.index.0;
        while o + 8 <= GC_HEADER_SIZE + f.index.1 {
            derived.push((o, format::DERIVED_STORE_INDEX));
            o += 8;
        }
    }
    for &(o, k) in &derived {
        if let Some(word) = bytes.get_mut(o..o + 8) {
            word.fill(0);
        }
        slots.push((o as u32, Val::Derived(k)));
    }
    // Strings, big integers and buffers hold character or byte data, not words.
    use crate::gc::*;
    let data = matches!(
        t,
        GC_TYPE_STRING
            | GC_TYPE_BIGINT
            | GC_TYPE_BUFFER
            | GC_TYPE_BUFFER_ARRAY_BUFFER
            | GC_TYPE_BUFFER_SHARED_ARRAY_BUFFER
            | GC_TYPE_TYPED_ARRAY
    );
    if data || no_form(t) {
        return;
    }
    let mut o = GC_HEADER_SIZE;
    while o + 8 <= bytes.len() {
        if !slots.iter().any(|s| s.0 as usize == o) {
            let bits = le(bytes, o);
            let ok = match w.val(bits) {
                Ok(Val::Scalar(_)) => true,
                Ok(Val::Ext { region, .. }) => region != format::REGION_TLS,
                _ => false,
            };
            if !ok {
                stop(stops, format::STOP_RAW_HEAP_WORD, o, bits);
            }
        }
        o += 8;
    }
}

/// The module-path registry entries the module inits added, as a replay record.
fn registry_record(w: &Where, stops: &mut Vec<Stop>) -> Vec<format::RegRec> {
    let mut out = Vec::new();
    for (key, init, status, exports, factory) in crate::module_require::snap_image_registry() {
        let mut bad = |value: u64| {
            stops.push(Stop {
                cell: out.len() as u32,
                obj_type: 0,
                kind: format::STOP_REGISTRY,
                off: 0,
                value,
            });
        };
        let val = |bits: Option<u64>| -> Result<Option<Val>, u64> {
            match bits.map(|b| w.val(b)) {
                None => Ok(None),
                Some(Ok(v @ (Val::Scalar(_) | Val::Cell { .. } | Val::Ext { .. }))) => Ok(Some(v)),
                Some(Ok(_)) => Err(bits.unwrap_or(0)),
                Some(Err(b)) => Err(b),
            }
        };
        let init_off = init
            .filter(|a| *a >= w.exe.0 && *a < w.exe.1)
            .map(|a| (a - w.exe.0) as u64);
        let (Some(init_off), true, Ok(exports), Ok(factory)) = (
            init_off,
            status == 0 || status == 2,
            val(exports),
            val(factory),
        ) else {
            bad(u64::from(status));
            continue;
        };
        out.push(format::RegRec {
            key: key.into_bytes(),
            init_off,
            status,
            exports,
            factory,
        });
    }
    out
}

/// The executable's own TLS block: variant II places it at
/// `[fs - roundup(memsz, align), + memsz)`; the gap above it up to `fs` holds
/// the C library's blocks (errno among them), so it is not part of the window.
#[cfg(any(test, all(target_os = "linux", target_arch = "x86_64", target_env = "gnu")))]
fn tls_window(fs: usize, memsz: usize, align: usize) -> (usize, usize) {
    let lo = fs.saturating_sub(memsz.div_ceil(align.max(1)) * align.max(1));
    (lo, memsz.min(fs - lo))
}

// Image replay R1 (enter step 3b): the shape records minted again with positioned counters.
mod replay;
