//! Stable natural merge sort of an index permutation. Only integer indices
//! move while a comparator runs; the caller keeps the source values rooted.
//! Unlike slice::sort_by, inconsistent comparators cannot cause a panic or
//! lose elements. The caller also owns scratch so JS throws cannot leak Vecs.

const MIN_RUN: usize = 32;

#[derive(Clone, Copy, Default)]
struct Run {
    start: usize,
    len: usize,
}

/// Sort an identity permutation with caller-provided, equally-sized scratch.
/// `le(a, b)` compares source values at indices a and b, treating equality as
/// true. Indices never escape the input range, even for inconsistent `le`.
pub(super) fn sort_indices(
    order: &mut [u32],
    scratch: &mut [u32],
    mut le: impl FnMut(u32, u32) -> bool,
) {
    let len = order.len();
    assert!(len <= u32::MAX as usize && scratch.len() >= len);
    // The collapse invariant makes pending lengths grow at least as fast as
    // Fibonacci numbers. 64 entries suffice for every u32-sized JS array.
    let mut runs = [Run::default(); 64];
    let mut pending = 0;
    let mut start = 0;
    while start < len {
        let mut end = start + 1;
        if end < len {
            let ascending = le(order[start], order[end]);
            end += 1;
            while end < len && le(order[end - 1], order[end]) == ascending {
                end += 1;
            }
            if !ascending {
                // Strictly descending only: reversing equal elements would
                // break stability. NaN comparator results are equal upstream.
                order[start..end].reverse();
            }
        }
        let run_end = end.max(start.saturating_add(MIN_RUN).min(len));
        // Extend short runs with binary insertion. An index can stay in a
        // register across a callback; a copied heap value could not.
        for i in end..run_end {
            let key = order[i];
            let (mut lo, mut hi) = (start, i);
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                if le(order[mid], key) {
                    lo = mid + 1;
                } else {
                    hi = mid;
                }
            }
            order.copy_within(lo..i, lo + 1);
            order[lo] = key;
        }
        runs[pending] = Run {
            start,
            len: run_end - start,
        };
        pending += 1;
        start = run_end;

        // Restore BOTH three-run inequalities, including the one below the
        // top triple. Checking only the top triple can overflow a run stack
        // on adversarial run-length sequences.
        while pending > 1 {
            let mut i = pending - 2;
            if (i > 0 && runs[i - 1].len <= runs[i].len + runs[i + 1].len)
                || (i > 1 && runs[i - 2].len <= runs[i - 1].len + runs[i].len)
            {
                if runs[i - 1].len < runs[i + 1].len {
                    i -= 1;
                }
            } else if runs[i].len > runs[i + 1].len {
                break;
            }
            merge_at(order, scratch, &mut runs, &mut pending, i, &mut le);
        }
    }
    while pending > 1 {
        let mut i = pending - 2;
        if i > 0 && runs[i - 1].len < runs[i + 1].len {
            i -= 1;
        }
        merge_at(order, scratch, &mut runs, &mut pending, i, &mut le);
    }
}

fn merge_at(
    order: &mut [u32],
    scratch: &mut [u32],
    runs: &mut [Run],
    pending: &mut usize,
    i: usize,
    le: &mut impl FnMut(u32, u32) -> bool,
) {
    let start = runs[i].start;
    let mid = start + runs[i].len;
    let end = mid + runs[i + 1].len;
    runs[i].len += runs[i + 1].len;
    runs.copy_within(i + 2..*pending, i + 1);
    *pending -= 1;
    if le(order[mid - 1], order[mid]) {
        return;
    }
    // Only the left run needs a snapshot. While it has unconsumed values,
    // dest < right, so forward stores cannot overwrite the right run's next
    // unread index. Once the left run is empty, the right tail is in place.
    scratch[start..mid].copy_from_slice(&order[start..mid]);
    let (mut left, mut right, mut dest) = (start, mid, start);
    let (mut left_wins, mut right_wins) = (0, 0);
    while left < mid && right < end {
        // Left wins ties, preserving the order of equivalent source values.
        if le(scratch[left], order[right]) {
            order[dest] = scratch[left];
            left += 1;
            left_wins += 1;
            right_wins = 0;
        } else {
            order[dest] = order[right];
            right += 1;
            right_wins += 1;
            left_wins = 0;
        }
        dest += 1;
        // When one run repeatedly wins, find its next block with exponential
        // then binary search. This helps clustered and duplicate-heavy data
        // without imposing a binary search on each random-data comparison.
        if left_wins >= 7 && left < mid && right < end {
            let take = gallop_prefix(&scratch[left..mid], |item| le(item, order[right]));
            order[dest..dest + take].copy_from_slice(&scratch[left..left + take]);
            left += take;
            dest += take;
            left_wins = 0;
        } else if right_wins >= 7 && left < mid && right < end {
            // Strictly less on the right: ties must stay behind the left run.
            let take = gallop_prefix(&order[right..end], |item| !le(scratch[left], item));
            order.copy_within(right..right + take, dest);
            right += take;
            dest += take;
            right_wins = 0;
        }
    }
    order[dest..dest + mid - left].copy_from_slice(&scratch[left..mid]);
}

fn gallop_prefix(values: &[u32], mut belongs: impl FnMut(u32) -> bool) -> usize {
    let (mut lo, mut probe) = (0, 0usize);
    while probe < values.len() && belongs(values[probe]) {
        lo = probe + 1;
        probe = probe.saturating_mul(2).saturating_add(1);
    }
    let mut hi = probe.min(values.len());
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if belongs(values[mid]) {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

#[cfg(test)]
mod tests {
    use super::sort_indices;

    fn check(values: &[i32]) -> usize {
        let mut actual: Vec<u32> = (0..values.len() as u32).collect();
        let mut expected = actual.clone();
        expected.sort_by_key(|&i| values[i as usize]);
        let mut calls = 0;
        sort_indices(&mut actual, &mut vec![0; values.len()], |a, b| {
            calls += 1;
            values[a as usize] <= values[b as usize]
        });
        assert_eq!(actual, expected, "stable order for length {}", values.len());
        calls
    }

    #[test]
    fn stable_across_distributions_and_run_boundaries() {
        let mut state = 289u32;
        for n in [0, 1, 2, 3, 7, 31, 32, 33, 63, 64, 65, 127, 1023, 4096] {
            let random: Vec<i32> = (0..n)
                .map(|_| {
                    state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                    (state >> 16) as i32
                })
                .collect();
            check(&random);
            check(&(0..n).map(|i| i as i32).collect::<Vec<_>>());
            check(&(0..n).map(|i| -(i as i32)).collect::<Vec<_>>());
            check(&vec![1; n]);
            check(&random.iter().map(|x| x % 7).collect::<Vec<_>>());
            check(&(0..n).map(|i| -((i / 3) as i32)).collect::<Vec<_>>());
            check(&(0..n).map(|i| (i % 73) as i32).collect::<Vec<_>>());
            check(&(0..n).map(|i| i.min(n - i) as i32).collect::<Vec<_>>());
        }
    }

    #[test]
    fn exhaustive_duplicate_permutations_are_stable() {
        for n in 0..=8 {
            for mut encoded in 0..3usize.pow(n) {
                let mut values = vec![0; n as usize];
                for value in &mut values {
                    *value = (encoded % 3) as i32;
                    encoded /= 3;
                }
                check(&values);
            }
        }
    }

    #[test]
    fn linear_comparisons_for_natural_runs() {
        for n in [33, 1000, 100_000] {
            assert_eq!(check(&(0..n).collect::<Vec<_>>()), (n - 1) as usize);
            assert_eq!(check(&(0..n).rev().collect::<Vec<_>>()), (n - 1) as usize);
            assert_eq!(check(&vec![0; n as usize]), (n - 1) as usize);
        }
    }

    #[test]
    fn inconsistent_comparators_preserve_every_index_and_terminate() {
        for n in [3, 33, 1024, 10000] {
            for mode in 0..4 {
                let mut order: Vec<u32> = (0..n).collect();
                let mut calls = 0usize;
                sort_indices(&mut order, &mut vec![0; n as usize], |a, b| {
                    calls += 1;
                    assert!(calls < n as usize * 100);
                    match mode {
                        0 => true,
                        1 => false,
                        2 => calls % 2 == 0,
                        _ => (a + 1) % 3 == b % 3,
                    }
                });
                order.sort_unstable();
                assert_eq!(order, (0..n).collect::<Vec<_>>());
            }
        }
    }

    #[test]
    fn uneven_runs_keep_merges_balanced() {
        // Alternating lengths with nested near-Fibonacci groups stress both
        // the top and the lower run-stack collapse inequalities.
        let lengths = [
            161, 97, 63, 33, 31, 65, 129, 4097, 2049, 1025, 513, 257, 129, 65, 33,
        ];
        let mut values = Vec::new();
        for round in 0..20 {
            for &len in &lengths {
                let base = -((values.len() + round * 13) as i32);
                values.extend((0..len).map(|i| base + i as i32));
            }
        }
        assert!(check(&values) < values.len() * 30);
    }

    #[test]
    fn large_disjoint_runs_skip_blocks_without_losing_stability() {
        let values: Vec<i32> = (50_000..100_000).chain(0..50_000).collect();
        assert!(check(&values) < values.len() + 100);
        let duplicates: Vec<i32> = values.iter().map(|x| x / 100).collect();
        assert!(check(&duplicates) < duplicates.len() + 100);
    }

    #[test]
    fn gallop_boundaries() {
        for n in 0..128 {
            let values: Vec<u32> = (0..n).collect();
            for boundary in 0..=n {
                assert_eq!(
                    super::gallop_prefix(&values, |x| x < boundary),
                    boundary as usize
                );
            }
        }
    }
}
