//! Unordered shared numeric element access. Integer atomics read/store the
//! exact lane width; floats reinterpret the lane without changing rounding.
use super::*;
use std::sync::atomic::{AtomicU16, AtomicU32, AtomicU64, AtomicU8, Ordering::Relaxed};

/// Copy one shared lane into private storage without boxing or converting it.
/// Retain the existing unordered atomic access at exactly the element width.
///
/// # Safety
/// Source and destination have `size` bytes and the corresponding alignment;
/// destination is exclusively owned. Both spans stay live within `scope`.
pub(crate) unsafe fn copy_lane(
    source: *const u8,
    dest: *mut u8,
    size: usize,
    _scope: &crate::buffer::bytes::NoGc<'_>,
) {
    match size {
        1 => dest.write((&*source.cast::<AtomicU8>()).load(Relaxed)),
        2 => dest
            .cast::<u16>()
            .write((&*source.cast::<AtomicU16>()).load(Relaxed)),
        4 => dest
            .cast::<u32>()
            .write((&*source.cast::<AtomicU32>()).load(Relaxed)),
        8 => dest
            .cast::<u64>()
            .write((&*source.cast::<AtomicU64>()).load(Relaxed)),
        _ => unreachable!("typed element width"),
    }
}

pub(super) unsafe fn load(base: *const u8, kind: u8) -> f64 {
    match kind {
        KIND_INT8 => (&*base.cast::<AtomicU8>()).load(Relaxed) as i8 as f64,
        KIND_UINT8 | KIND_UINT8_CLAMPED => (&*base.cast::<AtomicU8>()).load(Relaxed) as f64,
        KIND_INT16 => (&*base.cast::<AtomicU16>()).load(Relaxed) as i16 as f64,
        KIND_UINT16 => (&*base.cast::<AtomicU16>()).load(Relaxed) as f64,
        KIND_INT32 => (&*base.cast::<AtomicU32>()).load(Relaxed) as i32 as f64,
        KIND_UINT32 => (&*base.cast::<AtomicU32>()).load(Relaxed) as f64,
        KIND_FLOAT16 => crate::array::canonical_raw_f64(f16_bits_to_f64(
            (&*base.cast::<AtomicU16>()).load(Relaxed),
        )),
        KIND_FLOAT32 => crate::array::canonical_raw_f64(f32::from_bits(
            (&*base.cast::<AtomicU32>()).load(Relaxed),
        ) as f64),
        KIND_FLOAT64 => crate::array::canonical_raw_f64(f64::from_bits(
            (&*base.cast::<AtomicU64>()).load(Relaxed),
        )),
        KIND_BIGINT64 => crate::value::js_nanbox_bigint(crate::bigint::js_bigint_from_i64(
            (&*base.cast::<AtomicU64>()).load(Relaxed) as i64,
        ) as i64),
        KIND_BIGUINT64 => crate::value::js_nanbox_bigint(crate::bigint::js_bigint_from_u64(
            (&*base.cast::<AtomicU64>()).load(Relaxed),
        ) as i64),
        _ => 0.0,
    }
}

pub(super) unsafe fn store(base: *mut u8, kind: u8, value: f64) {
    match kind {
        KIND_INT8 | KIND_UINT8 => {
            (&*base.cast::<AtomicU8>()).store(to_uint32_bits(value) as u8, Relaxed)
        }
        KIND_UINT8_CLAMPED => (&*base.cast::<AtomicU8>()).store(to_uint8_clamp(value), Relaxed),
        KIND_INT16 | KIND_UINT16 => {
            (&*base.cast::<AtomicU16>()).store(to_uint32_bits(value) as u16, Relaxed)
        }
        KIND_INT32 | KIND_UINT32 => {
            (&*base.cast::<AtomicU32>()).store(to_uint32_bits(value), Relaxed)
        }
        KIND_FLOAT16 => (&*base.cast::<AtomicU16>()).store(f64_to_f16_bits(value), Relaxed),
        KIND_FLOAT32 => (&*base.cast::<AtomicU32>()).store((value as f32).to_bits(), Relaxed),
        KIND_FLOAT64 => (&*base.cast::<AtomicU64>()).store(value.to_bits(), Relaxed),
        KIND_BIGINT64 | KIND_BIGUINT64 => {
            (&*base.cast::<AtomicU64>()).store(bigint::bigint_slot_bits(value), Relaxed)
        }
        _ => {}
    }
}
