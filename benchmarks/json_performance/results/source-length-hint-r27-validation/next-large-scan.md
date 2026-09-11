# Next large-token scan candidate — not applied

R25 profiles put parse_string_bytes self work at 35.4% on ASCII and 21.5% on Unicode. The current aarch64 find_neon scans 16 bytes and horizontally reduces each vector before advancing. A parser-only large-token helper could combine four 16-byte predicates before one reduction, then resolve the first matching block on a hit. This would reduce loop/reduction overhead while preserving the first quote/backslash/control result and bounded loads.

Do not dispatch solely from remaining-input length before the first probe: a short key at the start of a megabyte record array also has a large remaining slice. Keep the current first 16-byte probe so short keys/values take their existing path. Only after a no-hit probe and a sufficiently large remaining slice enter an outlined 64-byte scanner. Restrict the initial experiment to CONTROL=true/SURROGATE=false so stringify escape scans and quote-only nesting semantics do not change. The 16-byte final blocks and bounded scalar/word tail stay unchanged.

Required acceptance: compare every first-special position, all 256 byte values, adjacent specials and unaligned slices against the scalar predicate. Include actual guarded-page allocation endings for 0/1/15/16/17/63/64/65/127/128/129/255/256/257 lengths. Compare native/shadow compiler outputs and moving/protected GC behavior, then fresh changing ASCII/Unicode parse and record-array/small-string controls. No speedup is claimed from the source-level hypothesis.

Keep this independent of the canonical_u32_index roundtrip proposal, which targets a different indexed-read cost. Do not edit production while R27 is frozen.
