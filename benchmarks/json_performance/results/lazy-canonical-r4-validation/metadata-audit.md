# R4 native string metadata audit and validation plan

Corrected implementation 8a7a29d72 passed all 304 release JSON tests. Its matched
compiler/runtime-static/stdlib-static build passed in the primary own worktree.
The completed quiet focus rejects this candidate; see README.md. The public
baseline refresh
terminated before measurement because its quiet-host gate was not met.

R4 branch codex/json-lazy-canonical-r4 is in json-lazy-canonical-r4, based on
pushed R3 931f27a76. The primary own worktree json-merged-pr10022 is detached
at the same corrected source 8a7a29d72 to reuse its warm target. The PR stays
at a1b398402 and includes none of the canonical correction source.

## Contract

TapeEntry retains the same three integer fields and 12-byte/4-byte-alignment
contract. Container start/end links remain forward/backward tape indices.
For KEY/STRING only, link==STRING_NO_ESCAPES(1) is a positive proof that the
syntax scan found no backslash. Zero and unknown metadata retain the existing
body scan. No new managed object, root, pointer, header field, or collector policy.
This is narrower than a Unicode/canonical-object proof: UTF-8, separators,
duplicate keys, key order, numbers and subtree bounds still require their checks.

The native entry is pushed before scanning with the positive marker. The
existing backslash branch clears it. No Vec growth or managed allocation occurs
while the entry borrow is live. Syntax failure can leave a partial native entry,
but build_tape and both scratch callbacks expose entries only after the entire
syntax build succeeds. Scratch is cleared on all normal return paths.

Unknown metadata is deliberately conservative. Older/manual zero-filled leaf
entries continue through body validation; zero does not assert a plain string.
Only the exact positive value skips that scan, not arbitrary nonzero bits.

## Consumers inspected

All TapeEntry consumers found by rg across crates were inspected:

- json_tape::materialize_value_source: container links read inside kind match;
  keys/strings materialize by byte offset.
- count_object_fields and count_array_length: subtree jumps only for container
  kinds; string metadata cannot become an index.
- lazy_get and force_materialize_lazy: root kind checked, then child jumps are
  kind guarded. Sparse cache, moving roots and mutation paths are unchanged.
- json_tape/record_materialize::try_small_record: OBJ_START before reading link.
- json_tape/iterative: keys/strings use kind/offset; no leaf link interpretation.
- json/stringify_api: known root array link bounds the slice; stringify_lazy
  reads positive metadata only after a KEY/STRING match and endpoint checks.
- json_tape_store: copies/owns whole integer-only entries; no field decoding.
  Pointer-free side allocation and GC accounting are unchanged.
- tests and GC owned/side-allocation witnesses: container-link equality, entry
  equality and size/alignment checks retain their meaning. No leaf-zero production
  assumption was found. Existing owned/moving witnesses passed in validation.

## Coverage verified

Six new unit tests cover every JSON escape in first/subsequent keys and values,
plain Unicode/empty strings, SIMD prefixes, Vec growth, both depth specializations,
native transfer/borrowed scratch, malformed-input callback exclusion/scratch reuse,
raw WTF-8 distinction and conservative zero/unknown metadata admission. Existing
canonicalization, string decode, iterative materialization and GC witnesses remain.

Formatting, raw debt949, address classification, roots, store419, file-size and
registration gates pass. Store INIT/POINTER_FREE/ROOT/STACK remain human audited.
All 304 release JSON tests pass at the corrected source. The matched build and
162 candidate compiled comparisons pass; a separate dev
check was not run. The quiet focused replay is complete and rejects this candidate.

## Measurement plan and outcome

The plan was: after source freeze, run release JSON tests single-threaded, matched compiler
and both static wrappers, then the existing18-fixture/9-mode canonical probe plus
scan/retained/Unicode/escaped/malloc-cadence witnesses. Capture parse builder,
entry, string-constructor and lazy-normalizer disassembly. Do not infer identical
code from equal spans.

The quiet focus kept the five previous cases and added records_array_1m
parse:512:8:9, scan:256:8:9 and sparse:512:8:9, with candidate/main/PR arms. This
exposed parse work moved by the early native entry push, alongside stringify
savings. All 32 output checks and 216 trials passed. The comparison against main
and the PR runtime rejected R4; no expanded matrices were warranted.

## Cost risks to measure, not assume away

Moving the native push before validation changes register liveness and can grow
scratch for a malformed final string where the old scanner returned first. Each
backslash now also clears a native integer field. The validity/error result is
unchanged, but these are possible CPU/RSS costs; escaped parse rows and malformed
input pressure should be checked before acceptance. A returning scalar proof
would avoid the early push, at the possible cost of another live register. Do
not select that variant without generated-code and timing evidence.
