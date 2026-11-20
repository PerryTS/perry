`String.prototype.localeCompare` no longer allocates on a comparison (#10094).
The primary (case-insensitive) pass built two fresh lowercased `String`s with
`str::to_lowercase` and compared those, so every comparison paid two heap
allocations and two full Unicode case-mapping passes over both operands to
answer a question that is usually decided by the first character — and a sort
pays that O(n log n) times. It now walks the two lowercased scalar streams in
lockstep and stops at the first difference, with a byte-level loop for the
leading all-ASCII run. On the non-ASCII path, `locale_compare_canonical` also
stops materializing two NFC `String`s per comparison: an `is_nfc_quick` check
(a table lookup per scalar, no allocation) skips the rewrite for text that is
already NFC, which covers precomposed letters, CJK and emoji.

**The ordering is deliberately unchanged.** Perry ships no collation table and
no locale tailoring, and that stays true here — #10094 records the decision.
U+03A3 is the one scalar whose lowercase mapping is context-dependent (final
sigma `ς` vs medial `σ`); the walk reports it rather than guessing, and falls
back to `str::to_lowercase`, which implements the Final_Sigma rule.

Tests pin both halves of that claim.
`matches_the_allocating_reference_{on_every_pair,on_random_strings}` are
differential against the exact `to_lowercase`-materializing formulation this
replaces, over a corpus spanning ASCII, Latin-1 accented letters in both
spellings, CJK, emoji, combining marks, the two special case mappings and
WTF-8 lone surrogates, and assert the corpus reaches all three arms.
`locale_compare_is_a_strict_weak_ordering` proves reflexivity, antisymmetry and
transitivity over every triple, so `Array.prototype.sort` stays well-defined.
`canonical_equivalents_stay_equal` and `documented_guarantees_hold` cover
canonical equivalence, case-only differences, empty/prefix/long-common-prefix
pairs, and the documented divergence from ICU.

Measured on a quiet M1 mini (load ~1.7) against Node v26.5.1, base and fixed
runtimes built from the same tree with the same compiler package set, the two
arms and Node interleaved, three rounds per point:

| workload | n | base | fixed |
|---|---:|---:|---:|
| `sort-objects-locale-key-unicode` | 100 | 2.61× Node | **0.79× Node** |
| `sort-objects-locale-key-unicode` | 10,000 | 2.86× | **0.82×** |
| `sort-objects-locale-key-unicode` | 100,000 | 2.92× | **0.85×** |
| `sort-objects-locale-key-unicode` | 1,000,000 | TIMEOUT (>60 s; 7.26 s/run uncapped) | **0.98× (2.62 s)** |
| `string-locale-compare-unicode` | 1,000,000 | 6.80× | **4.54×** |
| `string-locale-compare-ascii` | 1,000,000 | 8.17× | **7.77×** |

Perry-side checksums are byte-identical to the base runtime at every size,
including at n=1,000,000 where the base only completes with the harness
timeout lifted, and including the sort checksums that differ from Node's by
design.

The `string-locale-compare-ascii` row barely moves because its remaining cost
is not the comparison. It calls `localeCompare(other, 'en-US')`, and a
`locales` argument makes codegen emit `js_string_validate_collator_args` on
every call, which re-runs `CanonicalizeLocaleList` plus the
`InitializeCollator` option reads. Dropping just that argument from the same
workload takes the base runtime from 290.3 ms to 61.0 ms at n=1,000,000, so
~79% of that benchmark is spec-side-effect validation rather than collation.
With the argument gone, the comparator's own gain is visible: 1.71× → 1.42×
Node on ASCII and 3.77× → 1.83× Node on Unicode.

Also corrects `docs/typescript-parity-gaps.md`, which listed both
`localeCompare()` and `toLocaleLowerCase()`/`toLocaleUpperCase()` as "Missing
(needs Intl)". All three are implemented. The new note, and the rewritten
`js_string_locale_compare` doc comment, state what `localeCompare` actually
guarantees — canonical equivalence, case-insensitive code point order, a
lowercase-first case tiebreak — and what it does not: no collation weights, no
locale tailoring, and an order that differs from Node for accented letters,
symbols and emoji, by design rather than by omission.
