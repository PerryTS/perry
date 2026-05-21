//! Trait + primitive `SH` impls. Split out of `stable_hash.rs` (no behavior
//! change). The `SH` trait and `tag` helper are `pub(super)` so sibling
//! modules can implement / call them; nothing here is part of the public API.

use super::StableHasher;

pub(super) trait SH {
    fn hash<H: StableHasher>(&self, h: &mut H);
}

#[inline]
pub(super) fn tag<H: StableHasher>(h: &mut H, t: u16) {
    h.write(&t.to_le_bytes());
}

impl SH for bool {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        h.write(&[*self as u8]);
    }
}

impl SH for u8 {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        h.write(&[*self]);
    }
}

impl SH for u32 {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        h.write(&self.to_le_bytes());
    }
}

impl SH for usize {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        h.write(&(*self as u64).to_le_bytes());
    }
}

impl SH for i64 {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        h.write(&self.to_le_bytes());
    }
}

impl SH for f64 {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        // Canonicalize NaN to a single bit pattern so two NaN-producing
        // sources don't accidentally diverge on bit-payload alone.
        let bits = if self.is_nan() {
            f64::NAN.to_bits()
        } else if *self == 0.0 {
            // Treat +0.0 and -0.0 as equal (matches JS ===).
            0u64
        } else {
            self.to_bits()
        };
        h.write(&bits.to_le_bytes());
    }
}

impl SH for str {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        h.write(&(self.len() as u64).to_le_bytes());
        h.write(self.as_bytes());
    }
}

impl SH for String {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        SH::hash(self.as_str(), h);
    }
}

impl<T: SH> SH for Box<T> {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        SH::hash(self.as_ref(), h);
    }
}

impl<T: SH> SH for Vec<T> {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        h.write(&(self.len() as u64).to_le_bytes());
        for item in self {
            item.hash(h);
        }
    }
}

impl<T: SH> SH for Option<T> {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        match self {
            None => tag(h, 0),
            Some(v) => {
                tag(h, 1);
                v.hash(h);
            }
        }
    }
}

impl<A: SH, B: SH> SH for (A, B) {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        self.0.hash(h);
        self.1.hash(h);
    }
}

impl<A: SH, B: SH, C: SH> SH for (A, B, C) {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        self.0.hash(h);
        self.1.hash(h);
        self.2.hash(h);
    }
}
