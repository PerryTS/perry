//! One reservation; descriptors remain readable after payload retirement.
//! Classification copies facts, never pins payload or remote bitmap storage.
use super::{Kind, ALIGN};
use crate::arena::{HeapGeneration, HeapSpace};
use std::sync::atomic::{AtomicPtr, AtomicU64, AtomicUsize, Ordering::SeqCst};
use std::sync::{Mutex, OnceLock};
const MAX_PAYLOAD: usize = 1 << 40;
const MIN_PAYLOAD: usize = 16 * ALIGN;
const MAPPED: u64 = 1;
const UPDATING: u64 = 2;
#[repr(C, align(128))]
struct Descriptor {
    publication: AtomicU64,
    incarnation: AtomicU64,
    base: AtomicUsize,
    end: AtomicUsize,
    owner: AtomicU64,
    thread: AtomicUsize,
    starts: AtomicUsize,
    payload_cells: AtomicUsize,
}
struct Reservation {
    descriptors: usize,
    base: usize,
    len: usize,
    allocation: Mutex<()>,
}
static INITIAL: OnceLock<Option<Box<Reservation>>> = OnceLock::new();
static RESERVATION: AtomicPtr<Reservation> = AtomicPtr::new(std::ptr::null_mut());
static SERIAL: AtomicU64 = AtomicU64::new(1);
fn serial() -> u64 {
    let n = SERIAL.fetch_add(1, SeqCst);
    assert!(n < (1 << 48), "region incarnation exhausted");
    n << 16
}
fn space_bits(space: HeapSpace) -> u64 {
    (match space {
        HeapSpace::Unknown => 0,
        HeapSpace::NurseryEden => 1,
        HeapSpace::Survivor0 => 2,
        HeapSpace::Survivor1 => 3,
        HeapSpace::Longlived => 4,
        HeapSpace::Old => 5,
        HeapSpace::PromotedYoung => 6,
    }) << 4
}
fn decode_space(word: u64) -> HeapSpace {
    match (word >> 4) & 15 {
        1 => HeapSpace::NurseryEden,
        2 => HeapSpace::Survivor0,
        3 => HeapSpace::Survivor1,
        4 => HeapSpace::Longlived,
        5 => HeapSpace::Old,
        6 => HeapSpace::PromotedYoung,
        _ => HeapSpace::Unknown,
    }
}
fn kind_bits(kind: Kind) -> u64 {
    (match kind {
        Kind::NurseryBlock => 1,
        Kind::OldBlock => 2,
        Kind::LargeObject => 3,
        Kind::ByteStore => 4,
        Kind::Payload => 5,
    }) << 8
}
fn initial_payload_len(limit: Option<libc::rlim_t>) -> usize {
    #[cfg(test)]
    if sabotaged("sizing") {
        return MAX_PAYLOAD;
    }
    let len = match limit {
        Some(limit) if limit != libc::RLIM_INFINITY => MAX_PAYLOAD.min((limit / 4) as usize),
        _ => MAX_PAYLOAD,
    };
    len & !(ALIGN - 1)
}

#[cfg(test)]
static SNAPSHOT_READS: AtomicUsize = AtomicUsize::new(0);

impl Reservation {
    unsafe fn reserve() -> Option<Box<Self>> {
        let mut limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        let limit = (libc::getrlimit(libc::RLIMIT_AS, &mut limit) == 0).then_some(limit.rlim_cur);
        let mut len = initial_payload_len(limit);
        while len >= MIN_PAYLOAD {
            let prefix =
                (len / ALIGN * std::mem::size_of::<Descriptor>() + ALIGN - 1) & !(ALIGN - 1);
            let total = prefix + len;
            let raw = libc::mmap(
                std::ptr::null_mut(),
                total + ALIGN,
                libc::PROT_NONE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE,
                -1,
                0,
            );
            if raw == libc::MAP_FAILED {
                len = (len / 2) & !(ALIGN - 1);
                continue;
            }
            let address = raw as usize;
            let aligned = (address + ALIGN - 1) & !(ALIGN - 1);
            let before = aligned - address;
            if before != 0 {
                assert_eq!(libc::munmap(raw, before), 0);
            }
            let after = ALIGN - before;
            if after != 0 {
                assert_eq!(libc::munmap((aligned + total) as *mut _, after), 0);
            }
            let committed = libc::mmap(
                aligned as *mut _,
                prefix,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_FIXED | libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE,
                -1,
                0,
            );
            if committed == libc::MAP_FAILED {
                assert_eq!(libc::munmap(aligned as *mut _, total), 0);
                len = (len / 2) & !(ALIGN - 1);
                continue;
            }
            assert_eq!(libc::madvise(committed, prefix, libc::MADV_NOHUGEPAGE), 0);
            return Some(Box::new(Self {
                descriptors: aligned,
                base: aligned + prefix,
                len,
                allocation: Mutex::new(()),
            }));
        }
        None
    }
    fn descriptor(&self, slot: usize) -> &Descriptor {
        debug_assert!(slot < self.len / ALIGN);
        // Callers range-check addresses or bound their entire slot run first.
        // Demand-zero bytes are valid zero-valued atomics. Never decommit
        // this prefix: publication and incarnation must survive reuse.
        unsafe {
            &*((self.descriptors + slot * std::mem::size_of::<Descriptor>()) as *const Descriptor)
        }
    }
    fn slot(&self, addr: usize) -> Option<usize> {
        let relative = addr.wrapping_sub(self.base);
        (relative < self.len).then_some(relative >> 21)
    }
}
fn initialized() -> Option<&'static Reservation> {
    INITIAL
        .get_or_init(|| {
            let reservation = unsafe { Reservation::reserve() };
            if let Some(ref r) = reservation {
                RESERVATION.store(&**r as *const Reservation as *mut Reservation, SeqCst);
            }
            reservation
        })
        .as_deref()
}
fn existing() -> Option<&'static Reservation> {
    let ptr = RESERVATION.load(SeqCst);
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &*ptr })
    }
}
#[inline(always)]
fn generation_for_space(space: HeapSpace) -> HeapGeneration {
    match space {
        HeapSpace::NurseryEden | HeapSpace::Survivor0 | HeapSpace::Survivor1 => {
            HeapGeneration::Nursery
        }
        HeapSpace::Longlived => HeapGeneration::Longlived,
        HeapSpace::Old | HeapSpace::PromotedYoung => HeapGeneration::Old,
        HeapSpace::Unknown => HeapGeneration::Unknown,
    }
}

/// Only the owning OS thread writes its live descriptor. After mapped presence
/// and native-thread identity are checked, it can read exactly the fields it
/// needs without a remote snapshot or version recheck. No payload is accessed.
/// All fields stay atomic because remote proofs and slot allocation read them.
#[inline(always)]
fn owned_descriptor(addr: usize) -> Option<(&'static Descriptor, u64)> {
    let r = existing()?;
    let d = r.descriptor(r.slot(addr)?);
    let word = d.publication.load(SeqCst);
    if word & (MAPPED | UPDATING) != MAPPED {
        return None;
    }
    if d.thread.load(SeqCst) != crate::tls_hot::thread_identity() {
        return None;
    }
    Some((d, word))
}

#[inline(always)]
pub(crate) fn owned_generation(addr: usize) -> Option<HeapGeneration> {
    let (_, word) = owned_descriptor(addr)?;
    Some(generation_for_space(decode_space(word)))
}

#[inline(always)]
pub(crate) fn owned_space(addr: usize) -> Option<(HeapSpace, usize, *mut u64)> {
    let (d, word) = owned_descriptor(addr)?;
    let space = decode_space(word);
    (space != HeapSpace::Unknown).then(|| {
        (
            space,
            d.base.load(SeqCst),
            d.starts.load(SeqCst) as *mut u64,
        )
    })
}

pub(crate) fn owned_uniform_generation(base: usize, end: usize) -> Option<HeapGeneration> {
    let (d, word) = owned_descriptor(base)?;
    (end > base && end <= d.end.load(SeqCst)).then(|| generation_for_space(decode_space(word)))
}

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)] // Complete classifier result; phase 1 consumers use a subset.
pub(crate) struct Snapshot {
    pub(crate) base: usize,
    pub(crate) end: usize,
    pub(crate) owner: u64,
    pub(crate) thread: usize,
    pub(crate) incarnation: u64,
    pub(crate) space: HeapSpace,
    pub(crate) kind: u8,
    pub(crate) starts: usize,
    pub(crate) payload_cells: bool,
}
impl Snapshot {
    pub(crate) fn generation(self) -> HeapGeneration {
        generation_for_space(self.space)
    }
    /// Revalidate an explicit token. Bare addresses identify the new tenant.
    #[allow(dead_code)] // Phase 1 lifetime-token API, exercised by ABA tests.
    pub(crate) fn is_current(self, addr: usize) -> bool {
        classify(addr).is_some_and(|now| {
            now.incarnation == self.incarnation
                && now.base == self.base
                && now.end == self.end
                && now.owner == self.owner
        })
    }

    pub(crate) fn is_current_thread(self) -> bool {
        self.thread == crate::tls_hot::thread_identity()
    }
}
#[inline]
pub(crate) fn classify(addr: usize) -> Option<Snapshot> {
    classify_between(addr, || {})
}
#[inline(always)]
fn classify_between(addr: usize, between: impl FnOnce()) -> Option<Snapshot> {
    #[cfg(test)]
    SNAPSHOT_READS.fetch_add(1, SeqCst);
    let r = existing()?;
    let d = r.descriptor(r.slot(addr)?);
    let first = d.publication.load(SeqCst);
    if first & (MAPPED | UPDATING) != MAPPED {
        return None;
    }
    let snapshot = Snapshot {
        base: d.base.load(SeqCst),
        end: d.end.load(SeqCst),
        owner: d.owner.load(SeqCst),
        thread: d.thread.load(SeqCst),
        incarnation: d.incarnation.load(SeqCst),
        space: decode_space(first),
        kind: ((first >> 8) & 15) as u8,
        starts: d.starts.load(SeqCst),
        payload_cells: d.payload_cells.load(SeqCst) != 0,
    };
    #[cfg(test)]
    let snapshot = if sabotaged("incarnation") {
        Snapshot {
            incarnation: 0,
            ..snapshot
        }
    } else {
        snapshot
    };
    between();
    let second = d.publication.load(SeqCst);
    #[cfg(test)]
    let second = if sabotaged("version") { first } else { second };
    (first == second && addr >= snapshot.base && addr < snapshot.end).then_some(snapshot)
}
pub(crate) fn contains(addr: usize, len: usize) -> bool {
    let Some(end) = addr.checked_add(len) else {
        return false;
    };
    #[cfg(test)]
    if sabotaged("range") {
        return true;
    }
    classify(addr).is_some_and(|s| end <= s.end)
}
pub(super) unsafe fn map(kind: Kind, mapped: usize) -> *mut u8 {
    let Some(r) = initialized() else {
        return std::ptr::null_mut();
    };
    let _guard = r.allocation.lock().unwrap_or_else(|e| e.into_inner());
    let count = mapped / ALIGN;
    if count == 0 || count > r.len / ALIGN {
        return std::ptr::null_mut();
    }
    let mut first = 0;
    let mut run = 0;
    for slot in 0..r.len / ALIGN {
        let d = r.descriptor(slot);
        if d.publication.load(SeqCst) & MAPPED != 0 {
            run = 0;
            continue;
        }
        if run == 0 {
            first = slot;
        }
        run += 1;
        if run != count {
            continue;
        }
        let base = r.base + first * ALIGN;
        let data = libc::mmap(
            base as *mut _,
            mapped,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_FIXED | libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        );
        if data == libc::MAP_FAILED {
            return std::ptr::null_mut();
        }
        let incarnation = serial();
        for index in first..first + count {
            let d = r.descriptor(index);
            d.publication.store(UPDATING, SeqCst);
            d.base.store(base, SeqCst);
            d.end.store(base + mapped, SeqCst);
            d.owner.store(crate::agent::current_agent(), SeqCst);
            d.thread.store(crate::tls_hot::thread_identity(), SeqCst);
            d.incarnation.store(incarnation, SeqCst);
            d.starts.store(0, SeqCst);
            d.payload_cells.store(0, SeqCst);
            #[cfg(test)]
            if sabotaged("publish") {
                continue;
            }
            d.publication
                .store(serial() | kind_bits(kind) | MAPPED, SeqCst);
        }
        return data.cast();
    }
    std::ptr::null_mut()
}
pub(super) unsafe fn unmap(data: *mut u8, mapped: usize) {
    let r = existing().expect("region reservation missing");
    let _guard = r.allocation.lock().unwrap_or_else(|e| e.into_inner());
    let first = r.slot(data as usize).expect("foreign region free");
    assert!(
        mapped / ALIGN <= r.len / ALIGN - first,
        "region free outside reservation"
    );
    for index in first..first + mapped / ALIGN {
        let d = r.descriptor(index);
        assert_eq!(d.base.load(SeqCst), data as usize);
        #[cfg(test)]
        if sabotaged("retire") {
            continue;
        }
        d.publication.store(serial(), SeqCst);
    }
    let replacement = libc::mmap(
        data.cast(),
        mapped,
        libc::PROT_NONE,
        libc::MAP_FIXED | libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE,
        -1,
        0,
    );
    assert_eq!(replacement, data.cast(), "region retirement failed");
}
/// Existing register/retag/remove funnel owns active presence. Pool backing
/// remains mapped while active space/bitmap facts disappear.
pub(crate) fn set_space(base: usize, len: usize, space: HeapSpace, starts: Option<usize>) -> bool {
    let Some(r) = existing() else {
        return false;
    };
    let Some(first) = r.slot(base) else {
        return false;
    };
    let count = len.div_ceil(ALIGN);
    assert!(
        count <= r.len / ALIGN - first,
        "region metadata outside reservation"
    );
    let _guard = r.allocation.lock().unwrap_or_else(|e| e.into_inner());
    for index in first..first + count {
        let d = r.descriptor(index);
        let before = d.publication.load(SeqCst);
        assert!(before & MAPPED != 0 && d.base.load(SeqCst) == base);
        d.publication.store(before | UPDATING, SeqCst);
        if let Some(starts) = starts {
            d.starts.store(starts, SeqCst);
        }
        let kind = match space {
            HeapSpace::Old | HeapSpace::PromotedYoung | HeapSpace::Longlived if len > ALIGN => {
                Kind::LargeObject
            }
            HeapSpace::Old | HeapSpace::PromotedYoung | HeapSpace::Longlived => Kind::OldBlock,
            HeapSpace::Unknown => match (before >> 8) & 15 {
                2 => Kind::OldBlock,
                3 => Kind::LargeObject,
                _ => Kind::NurseryBlock,
            },
            _ => Kind::NurseryBlock,
        };
        d.publication.store(
            serial() | MAPPED | kind_bits(kind) | space_bits(space),
            SeqCst,
        );
    }
    true
}
pub(crate) fn note_payload_cell(addr: usize) {
    #[cfg(test)]
    if sabotaged("payload_cells") {
        return;
    }
    // The owner holds the allocation live across this call. An oversized
    // ArenaBlock has several slot records, all describing the same block.
    // Its summary must be visible through every one of those records.
    if let (Some(r), Some((d, _))) = (existing(), owned_descriptor(addr)) {
        let first = r
            .slot(d.base.load(SeqCst))
            .expect("owned extent outside reservation");
        for slot in first..first + (d.end.load(SeqCst) - d.base.load(SeqCst)) / ALIGN {
            r.descriptor(slot).payload_cells.store(1, SeqCst);
        }
    }
}
#[cfg(test)]
fn sabotaged(mode: &str) -> bool {
    std::env::var("PERRY_R2_SABOTAGE").is_ok_and(|value| value == mode)
}
#[cfg(test)]
mod tests;
