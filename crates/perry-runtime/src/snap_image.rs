//! Image run: the runtime half of the execution-prefix image.
//!
//! A binary compiled with `PERRY_SNAP_IMAGE_RUN` calls [`js_snap_image_begin`]
//! first in `main` and [`js_snap_image_cut`] where the entry body starts, after
//! every static dependency has been initialised. With
//! `PERRY_SNAP_IMAGE_OUT=<file>` set, the cut collects, walks every cell the
//! allocators hold, writes the image file (see `format`) and exits before the
//! entry body runs. Without it the cut returns at once. No other binary
//! reaches this code: the calls are emitted only for the knob.

pub mod format;
#[cfg(test)]
mod format_read;
#[cfg(test)]
mod tests;

use format::{Cell, Image, Stop, Val, Word};
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

/// What `begin` saw: the writable data and the main thread's TLS block, and
/// the program's TLS slot ranges (data begin/end, bss begin/end).
pub(crate) struct Baseline {
    data: (usize, Vec<u8>),
    tls: (usize, Vec<u8>),
    program_tls: [usize; 4],
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
/// ends at the thread pointer).
fn executable_tls_size() -> usize {
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
                unsafe { *(data as *mut usize) = (ph.p_memsz as usize).div_ceil(align) * align };
            }
        }
        1
    }
    let mut size = 0usize;
    unsafe { libc::dl_iterate_phdr(Some(each), (&mut size as *mut usize).cast()) };
    size
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[no_mangle]
pub extern "C" fn js_snap_image_begin(
    tls_data_lo: usize,
    tls_data_hi: usize,
    tls_bss_lo: usize,
    tls_bss_hi: usize,
) {
    let mut fs = 0usize;
    // SAFETY: ARCH_GET_FS stores the thread pointer.
    unsafe { libc::syscall(libc::SYS_arch_prctl, 0x1003, &mut fs as *mut usize) };
    let tls_lo = fs.saturating_sub(executable_tls_size());
    let lo = std::ptr::addr_of!(__data_start) as usize;
    let hi = std::ptr::addr_of!(_end) as usize;
    let b = Box::new(Baseline {
        data: copy(lo, hi),
        tls: copy(tls_lo, fs),
        program_tls: [tls_data_lo, tls_data_hi, tls_bss_lo, tls_bss_hi],
    });
    BASELINE.store(Box::into_raw(b), Relaxed);
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[no_mangle]
pub extern "C" fn js_snap_image_cut() {
    let p = BASELINE.load(Relaxed);
    let Some(out) = std::env::var_os("PERRY_SNAP_IMAGE_OUT") else {
        return;
    };
    if p.is_null() {
        return;
    }
    BASELINE.store(std::ptr::null_mut(), Relaxed);
    // SAFETY: leaked in `begin`, never freed.
    let b = unsafe { &*p };
    crate::gc::js_gc_collect();
    let img = build(Some(b));
    let ok = std::fs::write(&out, format::encode(&img)).is_ok();
    std::process::exit(if ok { 0 } else { 70 });
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
    ) || crate::gc::gc_type_info(t).is_none()
}

/// Where an address lies, for classifying a word.
struct Where {
    /// (user address, header address, size), sorted by address.
    cells: Vec<(usize, usize, usize)>,
    sections: [(usize, usize); 3],
    exe: (usize, usize),
    tls: (usize, usize),
}

impl Where {
    fn val(&self, bits: u64) -> Result<Val, u64> {
        use crate::value::{BIGINT_TAG, POINTER_MASK, POINTER_TAG, STRING_TAG, TAG_MASK};
        let tag = bits & TAG_MASK;
        let (hi, bare, a) = if tag == POINTER_TAG || tag == STRING_TAG || tag == BIGINT_TAG {
            ((bits >> 48) as u16, false, (bits & POINTER_MASK) as usize)
        } else if (0x10000..(1u64 << 47)).contains(&bits) {
            (0, true, bits as usize)
        } else {
            return Ok(Val::Scalar(bits));
        };
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
        for (k, s) in self.sections.iter().enumerate() {
            if within(*s) {
                return Ok(Val::Ext {
                    hi,
                    bare,
                    region: k as u8,
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
        Ok(Val::Opaque(bits))
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
pub(crate) fn build(base: Option<&Baseline>) -> Image {
    use crate::gc::{GcHeader, GC_HEADER_SIZE};
    let headers = cell_headers();
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
            sections: [
                sect(&__start_perry_strings, &__stop_perry_strings),
                sect(&__start_perry_layouts, &__stop_perry_layouts),
                sect(&__start_perry_atoms, &__stop_perry_atoms),
            ],
            exe: sect(&__executable_start, &_end),
            tls: base.map_or((0, 0), |b| (b.tls.0, b.tls.0 + b.tls.1.len())),
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
            crate::gc::image_cells::visit_cell_slots_raw(hp, &mut |slot, bits| {
                let off = slot - h;
                let Some(word) = bytes.get_mut(off..off + 8) else {
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
        let p = b.program_tls;
        let in_program = |a: usize| (a >= p[0] && a < p[1]) || (a >= p[2] && a < p[3]);
        for (region, (lo, old)) in [(format::WORD_DATA, &b.data), (format::WORD_TLS, &b.tls)] {
            for (i, o) in old.chunks_exact(8).enumerate() {
                let a = lo + i * 8;
                if a >= own && a < own + 8 || (region == format::WORD_TLS && !in_program(a)) {
                    continue;
                }
                // SAFETY: the range the baseline copied.
                let now = unsafe { std::ptr::read_volatile(a as *const u64) };
                if now == u64::from_le_bytes(o.try_into().unwrap_or([0; 8])) {
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
    img.pending = format::PENDING;
    img
}
