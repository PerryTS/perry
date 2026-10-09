//! Per-visit word shape shared by full marking and remembering. No ownership
//! or liveness is implied by decoding; the two consumers retain their gates.

use super::*;

#[derive(Clone, Copy)]
pub(in crate::gc) struct FieldWord {
    addr: usize,
    raw: bool,
}

impl FieldWord {
    #[inline(always)]
    pub(in crate::gc) fn decode(bits: u64) -> Option<Self> {
        let tag = bits & TAG_MASK;
        if tag == POINTER_TAG || tag == STRING_TAG || tag == BIGINT_TAG {
            let addr = (bits & POINTER_MASK) as usize;
            return (addr != 0).then_some(Self { addr, raw: false });
        }
        // Preserve the marker's wider raw-word domain. The barrier consumer
        // below additionally requires its own floor/alignment/range proof.
        if !(0x1000..=0x0000_FFFF_FFFF_FFFF).contains(&bits) {
            return None;
        }
        Some(Self {
            addr: bits as usize,
            raw: true,
        })
    }

    #[inline(always)]
    pub(in crate::gc) fn addr(self) -> usize {
        self.addr
    }

    #[inline(always)]
    pub(in crate::gc) fn needs_tracking(self) -> bool {
        if self.raw {
            if self.addr < 0x10000 || self.addr & 0x7 != 0 {
                return false;
            }
            crate::gc::barrier::raw_child_needs_tracking(self.addr)
        } else {
            crate::gc::barrier::remembered_child_needs_tracking(self.addr)
        }
    }
}
