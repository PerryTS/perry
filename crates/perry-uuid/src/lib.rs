//! The two UUID versions Perry generates. Features default to empty: mail
//! selects v4; node:crypto selects v4+v7. Entropy comes from the OS, drawn
//! through a per-thread cache the way Node's `randomUUID` buffers it (#10523):
//! one `getrandom` fills 128 UUIDs' worth of bytes instead of one syscall per
//! UUID. `v4_uncached` is Node's `{ disableEntropyCache: true }`.
#[cfg(any(feature = "v4", feature = "v7"))]
mod entropy {
    use std::cell::RefCell;

    /// Node's `kBatchSize`: UUIDs served per entropy refill.
    const BATCH: usize = 128;
    const POOL_BYTES: usize = 16 * BATCH;

    struct Pool {
        bytes: [u8; POOL_BYTES],
        next: usize,
    }

    thread_local! {
        // Per-thread, so perry/thread agents never contend or share bytes.
        static POOL: RefCell<Pool> = const {
            RefCell::new(Pool { bytes: [0; POOL_BYTES], next: POOL_BYTES })
        };
    }

    #[cfg(test)]
    thread_local! {
        pub(crate) static FILLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    fn fill(buf: &mut [u8]) {
        #[cfg(test)]
        FILLS.with(|n| n.set(n.get() + 1));
        getrandom::fill(buf).expect("OS entropy unavailable for UUID generation");
    }

    pub(crate) fn fresh() -> [u8; 16] {
        let mut bytes = [0; 16];
        fill(&mut bytes);
        bytes
    }

    /// Hands out each pooled byte once. Falls back to a fresh draw while the
    /// thread's locals are being torn down.
    pub(crate) fn cached() -> [u8; 16] {
        POOL.try_with(|pool| {
            let mut pool = pool.borrow_mut();
            if pool.next == POOL_BYTES {
                fill(&mut pool.bytes);
                pool.next = 0;
            }
            let start = pool.next;
            pool.next += 16;
            let mut bytes = [0; 16];
            bytes.copy_from_slice(&pool.bytes[start..start + 16]);
            bytes
        })
        .unwrap_or_else(|_| fresh())
    }
}
/// A UUID in its 36-byte hyphenated form, formatted without allocating.
#[cfg(any(feature = "v4", feature = "v7"))]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hyphenated([u8; 36]);
#[cfg(any(feature = "v4", feature = "v7"))]
impl Hyphenated {
    fn new(bytes: [u8; 16]) -> Self {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        // Where each byte's two digits start, hyphens skipped. A constant
        // table instead of a running index with a per-byte hyphen test: the
        // loop unrolls to straight-line stores with no bounds checks, which was
        // a third of `randomUUID()`'s instructions (#10523).
        const AT: [usize; 16] = [0, 2, 4, 6, 9, 11, 14, 16, 19, 21, 24, 26, 28, 30, 32, 34];
        let mut out = [b'-'; 36];
        for i in 0..16 {
            out[AT[i]] = HEX[(bytes[i] >> 4) as usize];
            out[AT[i] + 1] = HEX[(bytes[i] & 15) as usize];
        }
        Hyphenated(out)
    }
    pub fn as_str(&self) -> &str {
        // Only ASCII hex digits and hyphens are ever written.
        std::str::from_utf8(&self.0).unwrap()
    }
    /// The 36 ASCII bytes, without `as_str`'s UTF-8 validation, for a caller
    /// that copies them into a string of its own.
    pub fn as_bytes(&self) -> &[u8; 36] {
        &self.0
    }
}
#[cfg(any(feature = "v4", feature = "v7"))]
impl std::fmt::Display for Hyphenated {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
#[cfg(any(feature = "v4", feature = "v7"))]
impl std::fmt::Debug for Hyphenated {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self.as_str(), f)
    }
}
#[cfg(feature = "v4")]
fn v4_from(mut bytes: [u8; 16]) -> Hyphenated {
    bytes[6] = (bytes[6] & 15) | 0x40;
    bytes[8] = (bytes[8] & 63) | 0x80;
    Hyphenated::new(bytes)
}
#[cfg(feature = "v4")]
pub fn v4() -> Hyphenated {
    v4_from(entropy::cached())
}
#[cfg(feature = "v4")]
pub fn v4_uncached() -> Hyphenated {
    v4_from(entropy::fresh())
}
#[cfg(feature = "v7")]
#[derive(Default)]
struct Sequence {
    millis: u64,
    counter: u64,
}
#[cfg(feature = "v7")]
impl Sequence {
    // A 42-bit randomly seeded counter occupies rand_a and the high 30 bits
    // of rand_b. Keep the low 32 bits random. Roll the logical timestamp
    // forward on overflow; clock rollback never reverses generation order.
    fn next(&mut self, millis: u64, random: [u8; 16]) -> [u8; 16] {
        if millis > self.millis || self.counter == 0 {
            self.millis = millis;
            self.counter = u64::from_be_bytes(random[..8].try_into().unwrap()) & ((1 << 41) - 1);
        } else if self.counter == (1 << 42) - 1 {
            self.millis += 1;
            self.counter = 0;
        }
        self.counter += 1;
        let mut bytes = random;
        bytes[..6].copy_from_slice(&self.millis.to_be_bytes()[2..]);
        bytes[6] = 0x70 | ((self.counter >> 38) as u8 & 15);
        bytes[7] = (self.counter >> 30) as u8;
        bytes[8] = 0x80 | ((self.counter >> 24) as u8 & 63);
        bytes[9] = (self.counter >> 16) as u8;
        bytes[10] = (self.counter >> 8) as u8;
        bytes[11] = self.counter as u8;
        bytes
    }
}
#[cfg(feature = "v7")]
pub fn v7() -> Hyphenated {
    use std::{
        sync::Mutex,
        time::{SystemTime, UNIX_EPOCH},
    };
    static SEQUENCE: Mutex<Sequence> = Mutex::new(Sequence {
        millis: 0,
        counter: 0,
    });
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("UUID clock precedes Unix epoch")
        .as_millis() as u64;
    let random = entropy::cached();
    let bytes = SEQUENCE.lock().unwrap().next(millis, random);
    Hyphenated::new(bytes)
}
#[cfg(test)]
mod tests {
    #[cfg(feature = "v4")]
    fn assert_v4_layout(s: &str) {
        assert_eq!(s.len(), 36);
        assert_eq!(&s[14..15], "4");
        assert!(matches!(s.as_bytes()[19], b'8' | b'9' | b'a' | b'b'));
        for (i, c) in s.bytes().enumerate() {
            if matches!(i, 8 | 13 | 18 | 23) {
                assert_eq!(c, b'-');
            } else {
                assert!(c.is_ascii_hexdigit() && !c.is_ascii_uppercase());
            }
        }
    }
    // #10523: the table-driven formatter writes every byte's digits in RFC
    // 9562 order around the four hyphens; `as_bytes` is `as_str`'s bytes.
    #[cfg(any(feature = "v4", feature = "v7"))]
    #[test]
    fn hyphenated_layout_is_exact() {
        let bytes: [u8; 16] = std::array::from_fn(|i| (i as u8) * 17);
        let h = super::Hyphenated::new(bytes);
        assert_eq!(h.as_str(), "00112233-4455-6677-8899-aabbccddeeff");
        assert_eq!(h.as_bytes(), h.as_str().as_bytes());
        let h =
            super::Hyphenated::new([0xf0, 0x0f, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 0xff]);
        assert_eq!(h.as_str(), "f00f0102-0304-0506-0708-090a0b0c0dff");
    }
    #[cfg(feature = "v4")]
    #[test]
    fn v4_layout_and_fresh_entropy() {
        let a = super::v4();
        let b = super::v4();
        assert_v4_layout(a.as_str());
        assert_v4_layout(super::v4_uncached().as_str());
        assert_eq!(format!("{a}"), a.as_str());
        assert_ne!(a, b);
    }
    // #10523: one OS draw per 128 cached UUIDs, every UUID distinct across
    // refills, and the uncached path never consumes the pool. Runs on its own
    // thread so the pool and counter start empty whatever the test harness
    // reuses.
    #[cfg(feature = "v4")]
    #[test]
    fn v4_draws_one_refill_per_128_uuids() {
        std::thread::spawn(|| {
            let fills = || super::entropy::FILLS.with(|n| n.get());
            let mut seen = std::collections::HashSet::new();
            let mut cached = |n: usize| {
                for _ in 0..n {
                    let id = super::v4();
                    assert_v4_layout(id.as_str());
                    assert!(seen.insert(id), "cached UUID repeated");
                }
            };
            cached(128);
            assert_eq!(fills(), 1);
            let uncached = super::v4_uncached();
            assert_eq!(fills(), 2);
            cached(128);
            assert_eq!(fills(), 3);
            cached(1);
            assert_eq!(fills(), 4);
            assert!(seen.insert(uncached));
        })
        .join()
        .unwrap();
    }
    #[cfg(feature = "v7")]
    #[test]
    fn v7_clock_rollback_and_counter_overflow() {
        let mut seq = super::Sequence::default();
        let a = seq.next(1700000000123, [3; 16]);
        let b = seq.next(1700000000123, [0; 16]);
        let c = seq.next(1, [0; 16]);
        assert!(a < b && b < c);
        assert_eq!(a[6] >> 4, 7);
        assert_eq!(a[8] >> 6, 2);
        assert_eq!(&a[..6], &1700000000123u64.to_be_bytes()[2..]);
        seq.counter = (1 << 42) - 1;
        let d = seq.next(1, [0; 16]);
        assert!(c < d);
        assert_eq!(seq.millis, 1700000000124);
    }
}
