//! Share exact nested function/class source bytes, without changing runtime
//! registration, strictness metadata or copying vs process-lifetime ownership.
use std::collections::HashMap;

use aho_corasick::{AhoCorasickBuilder, AhoCorasickKind};

use crate::{
    block::LlBlock,
    module::LlModule,
    types::{I64, I8},
};

const MIN_PATTERN_BYTES: usize = 4096;
/// Byte-sum cap on the all-parents Aho-Corasick intern. Nested function
/// source on a real bundle is *unique strings* whose lengths still sum to
/// the duplicated total (tsc: ~24 MB of overlapping slices of a ~6 MB
/// module). Exceeding this used to disable intern entirely (`plan` returned
/// one blob per function), which is how #10574 measured 24.3 MB of
/// `__cstring`. Over-budget modules now still share into the longest parent.
const MAX_PATTERN_BYTES: usize = 8 * 1024 * 1024;
const MAX_MATCHES: usize = 1_000_000;

#[derive(Clone)]
pub(super) struct SourceRange {
    global: String,
    offset: usize,
    pub(super) byte_len: usize,
}

impl SourceRange {
    pub(super) fn pointer(&self, block: &mut LlBlock) -> String {
        let base = format!("@{}", self.global);
        if self.offset == 0 {
            base
        } else {
            block.gep(I8, &base, &[(I64, &self.offset.to_string())])
        }
    }
}

pub(super) struct SourcePool<'a>(HashMap<&'a str, SourceRange>);

impl<'a> SourcePool<'a> {
    pub(super) fn emit(llmod: &mut LlModule, sources: impl Iterator<Item = &'a str>) -> Self {
        // Intern before building the matcher. Hash iteration never determines
        // emission order: first occurrence supplies every stable index.
        let mut unique = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for source in sources {
            if seen.insert(source) {
                unique.push(source);
            }
        }
        let bytes: Vec<&[u8]> = unique.iter().map(|source| source.as_bytes()).collect();
        let plan = plan(&bytes, MIN_PATTERN_BYTES, MAX_PATTERN_BYTES, MAX_MATCHES);
        let mut globals = HashMap::new();
        // Keep physical constants in original first-use order as well.
        for (idx, &(parent, _)) in plan.iter().enumerate() {
            if idx == parent {
                globals.insert(idx, llmod.add_string_constant(unique[idx]).0);
            }
        }
        Self(
            unique
                .into_iter()
                .enumerate()
                .map(|(idx, source)| {
                    let (parent, offset) = plan[idx];
                    (
                        source,
                        SourceRange {
                            global: globals[&parent].clone(),
                            offset,
                            byte_len: source.len(),
                        },
                    )
                })
                .collect(),
        )
    }

    pub(super) fn get(&self, source: &str) -> SourceRange {
        self.0
            .get(source)
            .expect("retained source was prepared")
            .clone()
    }
}

/// Each result names an input parent and an exact byte offset. Empty sources
/// and below-minimum modules keep independent byte ranges. Over-budget or
/// automaton-build failure still intern into the longest parent (#10574)
/// rather than emitting one copy per function.
fn plan(
    input: &[&[u8]],
    minimum_bytes: usize,
    maximum_bytes: usize,
    maximum_matches: usize,
) -> Vec<(usize, usize)> {
    let raw = || (0..input.len()).map(|idx| (idx, 0)).collect();
    let total = input
        .iter()
        .fold(0usize, |sum, bytes| sum.saturating_add(bytes.len()));
    if input.len() < 2 || total < minimum_bytes || input.iter().any(|bytes| bytes.is_empty()) {
        return raw();
    }
    if total > maximum_bytes || maximum_matches == 0 {
        return share_into_longest(input);
    }
    // A contiguous NFA avoids the potentially much larger dense DFA. Pattern
    // bytes and reported matches are bounded independently of source syntax.
    let Ok(automaton) = AhoCorasickBuilder::new()
        .kind(Some(AhoCorasickKind::ContiguousNFA))
        .build(input)
    else {
        return share_into_longest(input);
    };
    let mut order: Vec<usize> = (0..input.len()).collect();
    order.sort_by_key(|&idx| (std::cmp::Reverse(input[idx].len()), idx));
    let mut locations = vec![None; input.len()];
    let mut matches = 0;
    for parent in order {
        if locations[parent].is_some() {
            continue;
        }
        locations[parent] = Some((parent, 0));
        if matches == maximum_matches {
            continue;
        }
        for found in automaton.find_overlapping_iter(input[parent]) {
            let child = found.pattern().as_usize();
            // The matcher reports byte offsets. No UTF-8 normalization,
            // NUL termination or original-module coordinate inference occurs.
            locations[child].get_or_insert((parent, found.start()));
            matches += 1;
            if matches == maximum_matches {
                break;
            }
        }
    }
    locations
        .into_iter()
        .map(|location| location.expect("every source has a parent"))
        .collect()
}

/// Nested `Function.prototype.toString` text is almost always a slice of the
/// longest function (the CJS factory / module wrapper). Searching each
/// remaining unique string in that one haystack recovers the 4× duplication
/// without building an automaton over every overlapping copy.
fn share_into_longest(input: &[&[u8]]) -> Vec<(usize, usize)> {
    let n = input.len();
    let mut result: Vec<(usize, usize)> = (0..n).map(|idx| (idx, 0)).collect();
    if n < 2 {
        return result;
    }
    let parent = (0..n)
        .min_by_key(|&idx| (std::cmp::Reverse(input[idx].len()), idx))
        .expect("n >= 2");
    let haystack = input[parent];
    for (idx, needle) in input.iter().copied().enumerate() {
        if idx == parent || needle.len() > haystack.len() {
            continue;
        }
        if let Some(offset) = find_bytes(haystack, needle) {
            result[idx] = (parent, offset);
        }
    }
    result
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if needle.len() > haystack.len() {
        return None;
    }
    AhoCorasickBuilder::new()
        .kind(Some(AhoCorasickKind::ContiguousNFA))
        .build(std::iter::once(needle))
        .ok()?
        .find(haystack)
        .map(|found| found.start())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verify(
        input: &[&[u8]],
        minimum: usize,
        maximum: usize,
        matches: usize,
    ) -> Vec<(usize, usize)> {
        let result = plan(input, minimum, maximum, matches);
        for (&expected, &(parent, offset)) in input.iter().zip(&result) {
            assert_eq!(expected, &input[parent][offset..offset + expected.len()]);
            assert_eq!(
                result[parent],
                (parent, 0),
                "parents must not form alias chains"
            );
        }
        result
    }

    #[test]
    fn nested_unicode_nul_binary_and_deterministic_ties() {
        let input: &[&[u8]] = &[
            b"inner",
            b"outer(inner)\0\xff",
            b"(inner)",
            "世界".as_bytes(),
            "function 世界() { return '世界'; }".as_bytes(),
            b"[inner]",
        ];
        let first = verify(input, 0, 1024, 100);
        assert_eq!(first[0], (1, 6));
        assert_eq!(first[2], (1, 5));
        assert_eq!(first[3], (4, 9));
        assert_eq!(first[5], (5, 0));
        for _ in 0..16 {
            assert_eq!(verify(input, 0, 1024, 100), first);
        }
    }

    #[test]
    fn budgets_and_empty_sources_keep_exact_fallbacks() {
        let input: &[&[u8]] = &[b"a", b"aa", b"aaa", b"aaaa", b"b"];
        for bound in [0, 1, 2, 3, 8, 100] {
            verify(input, 0, 1024, bound);
        }
        let raw: Vec<_> = (0..input.len()).map(|idx| (idx, 0)).collect();
        assert_eq!(verify(input, 1024, 2048, 100), raw);
        // Over the automaton byte budget: still share into the longest parent
        // (`aaaa`) instead of disabling intern. `b` is disjoint and stays a
        // blob. #10574: this is the tsc-sized path (24 MB nested / 8 MB cap).
        let over = verify(input, 0, 1, 100);
        assert_eq!(over[0], (3, 0));
        assert_eq!(over[1], (3, 0));
        assert_eq!(over[2], (3, 0));
        assert_eq!(over[3], (3, 0));
        assert_eq!(over[4], (4, 0));
        verify(&[b"", b"hello"], 0, 1024, 100);
        verify(&[], 0, 1024, 100);
    }

    #[test]
    fn over_budget_nested_function_source_shares_into_the_longest_parent() {
        let inner = b"function inner() { return 1; }";
        let mut outer = b"function outer() { ".to_vec();
        outer.extend_from_slice(inner);
        outer.extend_from_slice(b" }");
        let input: &[&[u8]] = &[&outer, inner];
        // `maximum_bytes` below `outer.len() + inner.len()` forces the
        // longest-parent fallback the 8 MiB production cap takes on tsc.
        let result = verify(input, 0, 8, 100);
        assert_eq!(result[0], (0, 0));
        assert_eq!(result[1].0, 0);
        assert_eq!(&outer[result[1].1..result[1].1 + inner.len()], inner);
    }

    #[test]
    fn emitted_pool_deduplicates_small_sources_without_a_matcher() {
        let mut module = LlModule::new("aarch64-apple-darwin");
        let pool = SourcePool::emit(&mut module, ["small", "other", "small", ""].into_iter());
        assert_eq!(pool.get("small").global, ".str.0");
        assert_eq!(pool.get("other").global, ".str.1");
        assert_eq!(pool.get("").global, ".str.2");
        assert_eq!(pool.0.len(), 3);
        assert_eq!(pool.get("").byte_len, 0);
        assert_eq!(
            module
                .to_ir()
                .lines()
                .filter(|line| line.contains("private unnamed_addr constant"))
                .count(),
            3
        );
    }

    #[test]
    fn emitted_pool_shares_nonzero_utf8_ranges_and_only_emits_parents() {
        let child = "function 世界() { return '\\0'; }";
        let parent = format!("/*{}\0*/{child}/*tail*/", "é".repeat(2200));
        let mut module = LlModule::new("aarch64-apple-darwin");
        let pool = SourcePool::emit(&mut module, [child, &parent, child].into_iter());
        let range = pool.get(child);
        assert_eq!(range.global, pool.get(&parent).global);
        assert_eq!(range.offset, parent.find(child).unwrap());
        assert_eq!(range.byte_len, child.len());
        assert_eq!(
            module
                .to_ir()
                .lines()
                .filter(|line| line.contains("private unnamed_addr constant"))
                .count(),
            1
        );
    }

    #[test]
    fn equal_parents_choose_first_occurrence() {
        let result = verify(&[b"middle", b"[middle]", b"(middle)"], 0, 1024, 100);
        assert_eq!(result, [(1, 1), (1, 0), (2, 0)]);
    }
}
