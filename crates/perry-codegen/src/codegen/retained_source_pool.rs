//! Share exact nested function/class source bytes, without changing runtime
//! registration, strictness metadata or copying vs process-lifetime ownership.
use std::collections::HashMap;

use aho_corasick::{AhoCorasickBuilder, AhoCorasickKind};

use perry_hir::types::FuncId;

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
    pub(super) global: String,
    pub(super) offset: usize,
    pub(super) byte_len: usize,
    pub(super) decoded_offset: Option<usize>,
    /// Offset in the pool's uncompressed blob. Compiler-only: the census
    /// (`PERRY_SOURCE_NEED_REPORT`) reports owners in decoded bytes.
    pub(super) pool_offset: usize,
}

impl SourceRange {
    pub(super) fn pointer(&self, block: &mut LlBlock) -> String {
        assert!(
            self.decoded_offset.is_none(),
            "compressed ranges belong in infos"
        );
        let base = format!("@{}", self.global);
        if self.offset == 0 {
            base
        } else {
            block.gep(I8, &base, &[(I64, &self.offset.to_string())])
        }
    }
}

pub(super) struct SourcePool<'a> {
    ranges: HashMap<&'a str, SourceRange>,
    /// Uncompressed blob bytes and the bytes the global stores.
    decoded_len: usize,
    physical_len: usize,
}

impl<'a> SourcePool<'a> {
    pub(super) fn emit(llmod: &mut LlModule, sources: impl Iterator<Item = &'a str>) -> Self {
        Self::emit_impl(llmod, sources, false)
    }

    pub(super) fn emit_compressed(
        llmod: &mut LlModule,
        sources: impl Iterator<Item = &'a str>,
    ) -> Self {
        Self::emit_impl(llmod, sources, true)
    }

    fn emit_impl(
        llmod: &mut LlModule,
        sources: impl Iterator<Item = &'a str>,
        compressed: bool,
    ) -> Self {
        // Intern before building the matcher. Hash iteration never determines
        // emission order: first occurrence supplies every stable index.
        let mut unique = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for source in sources {
            if seen.insert(source) {
                unique.push(source);
            }
        }
        if unique.is_empty() {
            return Self {
                ranges: HashMap::new(),
                decoded_len: 0,
                physical_len: 0,
            };
        }
        let bytes: Vec<&[u8]> = unique.iter().map(|source| source.as_bytes()).collect();
        let plan = plan(&bytes, MIN_PATTERN_BYTES, MAX_PATTERN_BYTES, MAX_MATCHES);
        // One physical blob per module. Maximal parents retain their stable
        // first-use order; every contained source becomes a byte range inside
        // that blob. This separates cold reflection text from hot literals and
        // needs no terminators because every consumer carries a byte length.
        let mut blob = String::new();
        let mut parent_offsets = HashMap::new();
        for (idx, &(parent, _)) in plan.iter().enumerate() {
            if idx == parent {
                parent_offsets.insert(idx, blob.len());
                blob.push_str(unique[idx]);
            }
        }
        let decoded_len = blob.len();
        let mut physical_len = blob.len();
        let (global, frame_offsets) = if compressed {
            let mut encoded = Vec::new();
            let mut frame_offsets = Vec::new();
            use std::io::Write;
            for block in blob
                .as_bytes()
                .chunks(perry_abi::RETAINED_SOURCE_BLOCK_BYTES)
            {
                frame_offsets.push(encoded.len());
                let mut encoder =
                    flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::new(6));
                encoder.write_all(block).expect("compress source into Vec");
                let payload = encoder.finish().expect("finish source into Vec");
                encoded.extend_from_slice(&(payload.len() as u32).to_le_bytes());
                encoded.extend_from_slice(&(block.len() as u32).to_le_bytes());
                encoded.extend_from_slice(&payload);
            }
            // Empty sources are not attached to infos. Keep pool queries
            // well-defined for compiler tests that include an empty source.
            if frame_offsets.is_empty() {
                frame_offsets.push(0);
            }
            physical_len = encoded.len();
            (
                llmod.add_compressed_retained_source_constant(&encoded).0,
                frame_offsets,
            )
        } else {
            (llmod.add_retained_source_constant(&blob).0, Vec::new())
        };
        let ranges = unique
            .into_iter()
            .enumerate()
            .map(|(idx, source)| {
                let (parent, offset) = plan[idx];
                let original = parent_offsets[&parent] + offset;
                let (offset, decoded_offset) = if compressed {
                    let block = (original / perry_abi::RETAINED_SOURCE_BLOCK_BYTES)
                        .min(frame_offsets.len() - 1);
                    (
                        frame_offsets[block],
                        Some(original - block * perry_abi::RETAINED_SOURCE_BLOCK_BYTES),
                    )
                } else {
                    (original, None)
                };
                (
                    source,
                    SourceRange {
                        global: global.clone(),
                        offset,
                        byte_len: source.len(),
                        decoded_offset,
                        pool_offset: original,
                    },
                )
            })
            .collect();
        Self {
            ranges,
            decoded_len,
            physical_len,
        }
    }

    pub(super) fn get(&self, source: &str) -> SourceRange {
        self.ranges
            .get(source)
            .expect("retained source was prepared")
            .clone()
    }
}

/// Prepare direct compressed info ranges and the unchanged legacy raw ranges.
/// All deduplication/indexing here is compiler-only; the image gets direct
/// frames and each info's own immutable source parts.
pub(super) fn prepare_ranges(
    llmod: &mut LlModule,
    module_prefix: &str,
    user_fn_source: &[(String, std::borrow::Cow<'_, str>, bool, FuncId)],
    classes: &HashMap<String, &perry_hir::Class>,
    class_ids: &HashMap<String, u32>,
    class_source_text: &HashMap<u32, perry_hir::SourceText>,
    strings_outlive_registry: bool,
) -> (Vec<(String, SourceRange, bool)>, Vec<(u32, SourceRange)>) {
    // Collect class sources in registration order before preparing the shared
    // source pool: a class can contain the exact bytes of a method/closure.
    let mut class_sources: Vec<(u32, &perry_hir::SourceText)> = Vec::new();
    for (class_name, class) in classes.iter() {
        if *class_name != class.name || class_name.starts_with("__AnonShape_") {
            continue;
        }
        let cid = match class_ids.get(class_name).copied() {
            Some(c) if c != 0 => c,
            _ => continue,
        };
        if let Some(src) = class_source_text.get(&cid) {
            class_sources.push((cid, src));
        }
    }
    class_sources.sort_by_key(|entry| entry.0);
    class_sources.dedup_by_key(|(cid, _)| *cid);

    // Unloadable image owners copy these ranges before publication. Permanent
    // image owners retain direct read-only info ranges.

    // #4101/#9413: mint source globals BEFORE `init_fn` borrows `llmod`.
    // Sharing changes only the backing bytes, never registration order, source
    // lengths, strictness flags, or the copying/static ownership contract.
    // A function whose own source is short is read raw: a fixed size rule, not
    // a hot/cold split. Its bytes are a second, raw copy of what the parent
    // frame also holds. Everything longer stays in bounded DEFLATE frames.
    let compressed_pool_member = |llmod: &LlModule, symbol: &str, source: &str| {
        !source.is_empty()
            && source.len() > perry_abi::RETAINED_SOURCE_RAW_MAX_BYTES
            && strings_outlive_registry
            && llmod.can_attach_fn_source(symbol)
    };
    let compressed_sources: Vec<&str> = user_fn_source
        .iter()
        .filter(|(symbol, source, _, _)| compressed_pool_member(llmod, symbol, source))
        .map(|(_, source, _, _)| source.as_ref())
        .collect();
    // A function whose text the compiler dropped (an empty text: no execution
    // can read it, see the driver's `source_need::drop_unread_source`) still
    // owns its strictness bit. An info carries that bit on its own; a body
    // registered through the copying table registers an empty range instead,
    // which `toString` treats as absent.
    let registers_empty = |llmod: &LlModule, symbol: &str, non_strict: bool| {
        non_strict && !(strings_outlive_registry && llmod.can_attach_fn_source(symbol))
    };
    let raw_sources: Vec<&str> = user_fn_source
        .iter()
        .filter(|(symbol, source, non_strict, _)| {
            !symbol.is_empty()
                && (!source.is_empty() || registers_empty(llmod, symbol, *non_strict))
                && !compressed_pool_member(llmod, symbol, source)
        })
        .map(|(_, source, _, _)| source.as_ref())
        .collect();
    let compressed_pool = SourcePool::emit_compressed(llmod, compressed_sources.into_iter());
    let source_pool = SourcePool::emit(
        llmod,
        raw_sources
            .into_iter()
            .chain(class_sources.iter().map(|(_, source)| source.as_str())),
    );
    let mut user_fn_source_constants: Vec<(String, SourceRange, bool)> = Vec::new();
    for (wrapper_sym, source_text, is_non_strict_ordinary, _) in user_fn_source {
        if wrapper_sym.is_empty() {
            continue;
        }
        let in_info = llmod.can_attach_fn_source(wrapper_sym);
        if source_text.is_empty() {
            if !*is_non_strict_ordinary {
                continue;
            }
            if in_info && llmod.mark_fn_non_strict_ordinary(wrapper_sym) {
                continue;
            }
        }
        let source = if compressed_pool_member(llmod, wrapper_sym, source_text) {
            compressed_pool.get(source_text)
        } else {
            source_pool.get(source_text)
        };
        if in_info
            && llmod.attach_fn_source(
                wrapper_sym,
                &source.global,
                source.offset,
                source.decoded_offset,
                source.byte_len,
                *is_non_strict_ordinary,
            )
        {
            continue;
        }
        user_fn_source_constants.push((wrapper_sym.clone(), source, *is_non_strict_ordinary));
    }

    // #9413: the same pre-allocation for retained class source text — also
    // before `init_fn` borrows `llmod`.
    let class_source_constants: Vec<(u32, SourceRange)> = class_sources
        .into_iter()
        .map(|(cid, source)| (cid, source_pool.get(source)))
        .collect();
    if super::source_need::active() {
        record_census(
            llmod,
            module_prefix,
            user_fn_source,
            &source_pool,
            &compressed_pool,
            &class_source_constants,
            class_source_text,
            strings_outlive_registry,
        );
    }
    (user_fn_source_constants, class_source_constants)
}

/// PERRY_SOURCE_NEED_REPORT: report the ranges chosen above in decoded bytes
/// (the raw pool, then the compressed pool's uncompressed blob). Nothing
/// emitted depends on this.
#[allow(clippy::too_many_arguments)]
fn record_census(
    llmod: &LlModule,
    module_prefix: &str,
    user_fn_source: &[(String, std::borrow::Cow<'_, str>, bool, FuncId)],
    source_pool: &SourcePool<'_>,
    compressed_pool: &SourcePool<'_>,
    class_source_constants: &[(u32, SourceRange)],
    class_source_text: &HashMap<u32, perry_hir::SourceText>,
    strings_outlive_registry: bool,
) {
    use super::source_need::{source_hash, StoredEntry, StoredKey};
    let raw_len = source_pool.decoded_len;
    let mut entries = Vec::new();
    for (symbol, text, _, fid) in user_fn_source {
        if symbol.is_empty() || text.is_empty() {
            continue;
        }
        let compressed = text.len() > perry_abi::RETAINED_SOURCE_RAW_MAX_BYTES
            && strings_outlive_registry
            && llmod.can_attach_fn_source(symbol);
        let offset = if compressed {
            raw_len + compressed_pool.get(text).pool_offset
        } else {
            source_pool.get(text).pool_offset
        };
        entries.push(StoredEntry {
            key: StoredKey::Func(*fid),
            symbol: symbol.clone(),
            offset,
            len: text.len(),
            hash: source_hash(text.as_bytes()),
        });
    }
    for (cid, r) in class_source_constants {
        let hash = class_source_text
            .get(cid)
            .map_or(0, |s| source_hash(s.as_str().as_bytes()));
        entries.push(StoredEntry {
            key: StoredKey::Class(*cid),
            symbol: format!("class#{cid}"),
            offset: r.pool_offset,
            len: r.byte_len,
            hash,
        });
    }
    super::source_need::record_module(
        module_prefix,
        raw_len + compressed_pool.decoded_len,
        source_pool.physical_len + compressed_pool.physical_len,
        entries,
    );
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

/// Single-needle search: `memmem` needs no automaton build per needle, which
/// the one-pattern Aho-Corasick this replaced paid once per function (#10681).
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    memchr::memmem::find(haystack, needle)
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
    fn emitted_pool_puts_small_sources_in_one_cold_blob_without_a_matcher() {
        let mut module = LlModule::new("aarch64-apple-darwin");
        let pool = SourcePool::emit(&mut module, ["small", "other", "small", ""].into_iter());
        assert_eq!(pool.get("small").global, ".perry.retained_source");
        assert_eq!(pool.get("other").global, pool.get("small").global);
        assert_eq!(pool.get("").global, pool.get("small").global);
        assert_eq!(pool.get("small").offset, 0);
        assert_eq!(pool.get("other").offset, 5);
        assert_eq!(pool.get("").offset, 10);
        assert_eq!(pool.ranges.len(), 3);
        assert_eq!(pool.get("").byte_len, 0);
        let ir = module.to_ir();
        assert_eq!(
            ir.lines()
                .filter(|line| line.contains(".perry.retained_source = private constant"))
                .count(),
            1
        );
        assert!(ir.contains("section \"__TEXT,__perry_src\""));
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
                .filter(|line| line.contains(".perry.retained_source = private constant"))
                .count(),
            1
        );
    }

    #[test]
    fn compressed_ranges_directly_name_frames_and_preserve_nested_offsets() {
        let child = "function nested() { return '世界😀'; }";
        let parent = format!("/*{}*/{child}/*{}*/", "é".repeat(2300), "tail".repeat(3000));
        let mut module = LlModule::new("x86_64-unknown-linux-gnu");
        let pool = SourcePool::emit_compressed(&mut module, [parent.as_str(), child].into_iter());
        let inner = pool.get(child);
        assert_eq!(pool.get(&parent).offset, 0);
        assert!(inner.offset > 0, "child must name a later frame directly");
        assert_eq!(
            inner.decoded_offset,
            Some(parent.find(child).unwrap() % perry_abi::RETAINED_SOURCE_BLOCK_BYTES)
        );
        assert_eq!(inner.byte_len, child.len());
        let ir = module.to_ir();
        assert!(ir.contains(".perry.retained_source.deflate = private constant"));
        assert!(ir.contains("section \".perry_src\""));
        assert!(
            !ir.contains(child),
            "compiled infos retain compressed bytes only"
        );
    }

    /// A function whose text the driver dropped (empty) keeps its kind: an
    /// info gets the non-strict bit without a source range; the copying
    /// registry gets an empty range carrying the bit. A strict one gets
    /// nothing.
    #[test]
    fn a_dropped_text_keeps_the_non_strict_bit_and_retains_no_bytes() {
        use std::borrow::Cow;
        let js_body = || {
            vec![
                (crate::types::PTR, "%this_closure".to_string()),
                (crate::types::I64, "%js_this".to_string()),
            ]
        };
        let sources = vec![
            ("body_sloppy".to_string(), Cow::Borrowed(""), true, 1u32),
            ("body_strict".to_string(), Cow::Borrowed(""), false, 2u32),
        ];
        let no_classes = HashMap::new();
        for outlive in [true, false] {
            let mut module = LlModule::new("x86_64-unknown-linux-gnu");
            module.define_function("body_sloppy", crate::types::DOUBLE, js_body());
            module.define_function("body_strict", crate::types::DOUBLE, js_body());
            let (constants, classes) = prepare_ranges(
                &mut module,
                "m_",
                &sources,
                &no_classes,
                &HashMap::new(),
                &HashMap::new(),
                outlive,
            );
            assert!(classes.is_empty());
            let flags = |body: &str| {
                let mut out = (0, false);
                module.note_fn_info(body, |f| out = (f.flags, f.source.is_some()));
                out
            };
            let bit = perry_abi::FN_NON_STRICT_ORDINARY;
            assert!(
                constants.is_empty(),
                "all output kinds carry kind in canonical infos"
            );
            assert_eq!(flags("body_sloppy"), (bit, false));
            assert_eq!(flags("body_strict"), (0, false));
        }
    }

    #[test]
    fn equal_parents_choose_first_occurrence() {
        let result = verify(&[b"middle", b"[middle]", b"(middle)"], 0, 1024, 100);
        assert_eq!(result, [(1, 1), (1, 0), (2, 0)]);
    }
}
