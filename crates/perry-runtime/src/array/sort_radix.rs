//! Stable byte distribution shared by numeric values and keyed payloads.
//! Histograms prove each destination index; there are no size thresholds.
pub(super) fn histogram<const N: usize>(counts: &mut [[usize; 256]; N], key: u64) {
    for (digit, count) in counts.iter_mut().enumerate() {
        count[((key >> (digit * 8)) & 255) as usize] += 1;
    }
}

/// Sort in caller-owned storage. Always return the answer in `data`.
/// No JS code, GC allocation, or safepoint occurs, including in `key`.
pub(super) fn radix<T: Copy, const N: usize>(
    data: &mut [T],
    scratch: &mut [T],
    counts: &mut [[usize; 256]; N],
    key: impl Fn(T) -> u64,
) {
    let n = data.len();
    assert_eq!(scratch.len(), n);
    let mut swapped = false;
    for (digit, count) in counts.iter_mut().enumerate() {
        if count.iter().any(|&c| c == n) {
            continue;
        }
        let mut at = 0;
        for c in count.iter_mut() {
            let size = *c;
            *c = at;
            at += size;
        }
        // The histogram was made from exactly these n keys; prefix sums
        // partition [0,n), and each bucket advances exactly its own count.
        let (from, to) = if swapped {
            (&*scratch, &mut *data)
        } else {
            (&*data, &mut *scratch)
        };
        for &item in from {
            let bucket = ((key(item) >> (digit * 8)) & 255) as usize;
            unsafe {
                *to.get_unchecked_mut(count[bucket]) = item;
            }
            count[bucket] += 1;
        }
        swapped = !swapped;
    }
    if swapped {
        data.copy_from_slice(scratch);
    }
}

pub(super) fn float_key(value: f64, descending: bool) -> Option<u64> {
    if value.is_nan() {
        return None;
    }
    // CompareArrayElements treats both zero signs as equal. Payload-carrying
    // keyed sorts preserve their identities; reversible primitive sorting
    // refuses -0 before calling this function.
    let bits = if value == 0.0 { 0 } else { value.to_bits() };
    let key = if bits >> 63 == 0 {
        bits ^ (1 << 63)
    } else {
        !bits
    };
    Some(if descending { !key } else { key })
}

pub(super) fn float_value(mut key: u64, descending: bool) -> f64 {
    if descending {
        key = !key;
    }
    f64::from_bits(if key >> 63 != 0 {
        key ^ (1 << 63)
    } else {
        !key
    })
}
