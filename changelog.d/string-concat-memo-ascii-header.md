**String concat: stop re-scanning bytes for ASCII-ness twice, and stop
building a scratch buffer just to hash it** — 640-distinct short-string
concat down 26.5% (548 → 403 instructions/concat), a memo-ineligible 73-byte
concat down 26.8% (644 → 471), a 100%-memo-hit workload unchanged (319 →
318, within noise), measured with a differential instruction-count probe
(median-of-7, base vs arm built in the same session; bare-loop control flat
at ~3 in both).

`concat_byte_parts` (the `s + t` fast path for two statically-typed string
operands, `perry-runtime/src/string/concat.rs`) had three defects, all in the
same neighborhood:

1. It scanned both operands for ASCII-ness via `bytes_all_ascii` up front,
   then scanned them *again* on the heap path via `l_slice.is_ascii() &&
   r_slice.is_ascii()` — with the first scan's answer (`both_ascii`) sitting
   in scope, unused. Fixed by computing the bit once, in the caller (the new
   `str_bytes_ascii_from_jsvalue`, a sibling of `str_bytes_from_jsvalue`),
   and threading it through.
2. `bytes_all_ascii` scanned byte-at-a-time (`.iter().all(|&b| b < 0x80)`).
   `<[u8]>::is_ascii()` inspects the same bytes word-at-a-time and is total
   over arbitrary byte strings, valid or not (see the soundness note below —
   that "total over arbitrary bytes" property is why it's the only sound
   choice here, not just the faster one). Switching to it — `bytes_all_ascii`'s
   body, and `str_bytes_ascii_from_jsvalue`'s scan — is most of this change's
   win on `long73`: that workload's improvement is mostly the scan itself
   getting faster over ~140 bytes/concat, not any trick that avoids it.
3. The short-concat memo's probe assembled both operands into a 12-byte stack
   buffer just to hand the hash/lookup helpers one contiguous slice. FNV-1a is
   a streaming hash (`fnv(a ++ b)` needs no buffer, just fold `a` then `b`),
   and the byte compare on a hit/miss can run in the same two parts against
   the cached entry. `concat_memo_hash_parts` / `concat_memo_slot_and_tag_parts`
   / `concat_memo_lookup_parts` replace the buffer with direct two-slice
   hashing and lookup; the single-slice `js_string_concat_value` ("prefix" +
   i) memo probe is now implemented in terms of the same two-slice
   primitives.

**An earlier version of this change also tried to skip the scan entirely**,
by reading a heap string's ASCII-ness straight off its header (`utf16_len ==
byte_len`, already computed at construction, so free). That is unsound, and
was caught in review before landing: Perry heap-string payloads are not
guaranteed valid UTF-8 (WTF-8 lone surrogates, `Buffer.toString` of
arbitrary bytes, FFI blobs — #6085), and a payload ending in a truncated
multi-byte lead byte can coincide on `utf16_len == byte_len` without being
ASCII — `compute_utf16_len_wtf8` charges a truncated lead its full nominal
unit count while the payload holds fewer bytes than that sequence declares
(`[0xC3]`, a lone 2-byte lead, records `utf16_len == 1 == byte_len`;
`string/compare.rs`'s `utf16_cmp_bytes` doc names the identical hazard and
pins the identical corpus for its own ASCII fast path — this change's
`str_bytes_ascii_from_jsvalue` doc now cross-references it). The header
check survives only as a NEGATIVE filter: `utf16_len != byte_len` soundly
proves non-ASCII with no scan needed, because `compute_utf16_len_wtf8`
counts exactly one unit per byte for any run of bytes `< 0x80` — the
contrapositive holds unconditionally, not just for well-formed input — but
`utf16_len == byte_len` is ambiguous and always falls back to a real
`is_ascii()` scan. `js_string_concat_value`'s memo-admission gate had the
identical exposure independently: `prefix_u16 == prefix_blen` was treated as
sufficient on its own and the `bytes_all_ascii` check right after it deleted
as redundant with it; restored, with the same negative-filter-then-scan
reasoning documented at the call site.

No TypeScript-reachable path that constructs such a payload was found:
`Buffer.toString` (all seven encodings, `buffer/encode.rs`),
`TextDecoder.decode` (`text.rs`), and every `bun:ffi` string-returning path
(`read_cstring_value`, `dlopen.rs`'s `CString`/`cstring` conversions) all
validate via `str::from_utf8`/`from_utf8_lossy` (or are fed a Rust `&str`,
valid by construction) before ever calling `js_string_from_bytes` — `#609`
closed these same construction sites for a related UB hazard, and the fix
happens to guarantee well-formed output too. The fix stands regardless:
relying on an invariant this tree already documents as unsound is the wrong
foundation, and `js_string_from_bytes` is a `pub extern "C"` entry point
whose own contract must hold for any bytes, whether or not today's call
graph happens to always validate first. Regression tests are therefore
Rust-level, against hand-built malformed `StringHeader`s — the same
technique `string/compare.rs`'s own corpus and `tests_guard_page.rs` already
use — rather than a gap test:
`ascii_probe_falls_back_to_a_scan_when_the_header_lies`,
`concat_memo_declines_a_prefix_whose_header_lies_about_being_ascii`,
`concat_box_reports_not_well_formed_for_a_malformed_operand_either_side`
(`perry-runtime/src/string/tests.rs`) — all three fail against the reverted
(unsound) code, confirmed by temporarily reintroducing it and reverting
back.

The memo probe's own break-even hit rate (governed by defect 3's mechanics —
hash/lookup/admit cost — not by the ASCII determination defects 1/2 changed)
barely moved between the unsound and sound paths: ~54% with the unsound
header shortcut, ~50.6% with the sound negative-filter-then-scan, both
measured by forcing the governor on/off via a temporary env knob (since
removed). `MEMO_MIN_HIT_SHIFT` stays `1` (50%, still the closest
power-of-two floor to either number) — it moved from the original `2` (25%,
chosen as a plausible fraction, never measured against the probe's own cost)
in this same change, which is what made the floor worth re-deriving at all.

Covered by `test-files/test_gap_string_concat_memo_ascii_header.ts`
(byte-for-byte against node): ASCII boundary lengths crossing the SSO (5) and
memo (12) ceilings, 2/3/4-byte non-ASCII operands, a surrogate pair formed
*across* the join boundary and one that deliberately isn't (reverse order),
empty operands, and repeated-identical concats (both split two different
ways) to exercise the memo's "seen twice" admission and its `===` identity.
GC stress (`PERRY_GC_SCHEDULE_SEED=1` and `=42`, `RATE=1`,
`PROTECT_FROMSPACE=1`, `VERIFY_EVACUATION=1`, `FROMSPACE_SCAN_ABORT=1`) on a
concat-heavy fixture ran 117,923 copying minors and quarantined 14,739
retired from-space sets with no fault and output still matching node,
confirming the memo's GC roots (`scan_concat_memo_roots_mut`) survive
evacuation under the new two-slice storage.
