//! Shared sorting for cold paths (binary size).
//!
//! Every `slice::sort_by` / `sort_unstable_by_key` call site instantiates the
//! standard library's whole sort (driftsort or ipnsort plus its small-sort
//! networks, 3-8 KB each) for its own element type AND its own closure type.
//! The runtime had ~290 of them in every binary — ~270 KB, most of it on
//! paths that run a handful of times per process (GC diagnostics, stack-map
//! parsing, side-table rebuilds, reflection helpers).
//!
//! These helpers erase both: the comparison is a `&mut dyn FnMut`, and the
//! work is a single `sort_by` over `u32` positions, so all callers share one
//! instantiation. Applying the resulting permutation is generic but tiny.
//!
//! Cost: one indirect call per comparison plus an index buffer. Keep hot
//! sorts (`Array.prototype.sort`, typed-array sorts, property-enumeration
//! order) on the direct std methods.

use std::cmp::Ordering;

/// Stable order of `0..len` under `cmp(i, j)`. The single `sort_by`
/// instantiation every helper below shares.
#[inline(never)]
pub(crate) fn stable_order(len: usize, cmp: &mut dyn FnMut(usize, usize) -> Ordering) -> Vec<u32> {
    debug_assert!(len <= u32::MAX as usize);
    let mut order: Vec<u32> = (0..len as u32).collect();
    order.sort_by(|&a, &b| cmp(a as usize, b as usize));
    order
}

/// Reorder `v` so that `v[k]` becomes the old `v[order[k]]`, in place, by
/// following permutation cycles (no `Clone`/`Default` bound on `T`).
fn apply_order<T>(v: &mut [T], mut order: Vec<u32>) {
    for start in 0..order.len() {
        let mut current = start;
        // `order[current] == current` marks a finished (or fixed) position.
        while order[current] as usize != start {
            let next = order[current] as usize;
            v.swap(current, next);
            order[current] = current as u32;
            current = next;
        }
        order[current] = current as u32;
    }
}

/// Stable sort of `v` by `cmp` — drop-in for `v.sort_by(cmp)`, and a valid
/// replacement for `sort_unstable_by` (a stable order is also an unstable one).
pub(crate) fn sort_by<T>(v: &mut [T], mut cmp: impl FnMut(&T, &T) -> Ordering) {
    if v.len() < 2 {
        return;
    }
    let order = {
        let items: &[T] = v;
        stable_order(items.len(), &mut |a, b| cmp(&items[a], &items[b]))
    };
    apply_order(v, order);
}

/// Stable sort of `v` by `key` — drop-in for `sort_by_key` /
/// `sort_unstable_by_key`.
pub(crate) fn sort_by_key<T, K: Ord>(v: &mut [T], mut key: impl FnMut(&T) -> K) {
    sort_by(v, |a, b| key(a).cmp(&key(b)));
}

/// Sort `v` by an integer key — drop-in for `sort_unstable_by_key` when the
/// key fits in a `u64`, for paths that are warm enough to care about an
/// indirect call per comparison (startup stack-map parsing, promise reaction
/// order). Sorts `(key, position)` pairs with a direct comparison — one
/// instantiation shared by every caller — then permutes `v`. Ties keep their
/// original order.
pub(crate) fn sort_by_u64_key<T>(v: &mut [T], mut key: impl FnMut(&T) -> u64) {
    if v.len() < 2 {
        return;
    }
    debug_assert!(v.len() <= u32::MAX as usize);
    let pairs: Vec<(u64, u32)> = v
        .iter()
        .enumerate()
        .map(|(i, item)| (key(item), i as u32))
        .collect();
    let order = sorted_pair_positions(pairs);
    apply_order(v, order);
}

#[inline(never)]
fn sorted_pair_positions(mut pairs: Vec<(u64, u32)>) -> Vec<u32> {
    // Positions are unique, so sorting the pair is a stable key sort.
    pairs.sort_unstable();
    pairs.into_iter().map(|(_, position)| position).collect()
}

/// Ascending sort of an `Ord` slice — drop-in for `sort` / `sort_unstable`.
pub(crate) fn sort<T: Ord>(v: &mut [T]) {
    sort_by(v, |a, b| a.cmp(b));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_std_stable_sort_including_ties() {
        // Pseudo-random data with many equal keys: stability is observable.
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        for len in [0usize, 1, 2, 3, 7, 31, 32, 33, 257, 4096] {
            let items: Vec<(u8, u32)> = (0..len as u32)
                .map(|i| {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    ((seed % 11) as u8, i)
                })
                .collect();
            let mut expected = items.clone();
            expected.sort_by_key(|&(k, _)| k);
            let mut actual = items.clone();
            sort_by_key(&mut actual, |&(k, _)| k);
            assert_eq!(actual, expected, "len {len}");
            let mut rev_expected = items.clone();
            rev_expected.sort_by(|a, b| b.0.cmp(&a.0));
            let mut rev_actual = items;
            sort_by(&mut rev_actual, |a, b| b.0.cmp(&a.0));
            assert_eq!(rev_actual, rev_expected, "len {len} reversed");
        }
    }

    #[test]
    fn u64_key_sort_is_stable_and_matches_std() {
        let items: Vec<(u64, usize)> = (0..1000).map(|i| (((i * 7919) % 37) as u64, i)).collect();
        let mut expected = items.clone();
        expected.sort_by_key(|&(k, _)| k);
        let mut actual = items;
        sort_by_u64_key(&mut actual, |&(k, _)| k);
        assert_eq!(actual, expected);
    }

    #[test]
    fn sorts_non_clone_values() {
        struct Owned(String);
        let mut v: Vec<Owned> = ["pear", "apple", "fig", "apple"]
            .iter()
            .map(|s| Owned((*s).to_string()))
            .collect();
        sort_by(&mut v, |a, b| a.0.cmp(&b.0));
        let got: Vec<&str> = v.iter().map(|o| o.0.as_str()).collect();
        assert_eq!(got, ["apple", "apple", "fig", "pear"]);
    }
}
