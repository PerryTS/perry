# Correct source reuse for lazy stringify

Main `eee3881c4` and the tape-depth candidates copy untouched lazy-array source
without proving canonical string spelling or property enumeration. The original
[diagnostic](results/lazy-canonical-probe/README.md) demonstrates five inherited
failures: whitespace, Unicode escapes, slash escapes, duplicate keys and reversed
integer-like keys. Ordinary direct parsing produces the correct output.

R1 (`96df018b1e436d52aefc0eca8a55fcb5df5e2ee4`) validates compact separators,
canonical strings, unique keys and numeric-key ordering before source copying.
Ambiguous cases materialize before ordinary stringify. Admission uses only native
scratch and introduces no managed intermediate values or collector-policy changes.
All 297 JSON tests and 126 compiled candidate checks pass; additional main controls
expose escaped/nested duplicates and index-after-name ordering as inherited bugs.
[Validation, raw output and moving-GC witnesses](results/lazy-canonical-r1-validation/README.md).

R1 is parked because its [quiet focused replay](results/quiet-lazy-canonical-r1-main-focus-r9/README.md)
shows 87–101% slower record roundtrips than main and 119–144% slower than tape-depth
R2. All three sizes have 9/9 slower pairs. The eager heterogeneous stringify concern
remains +0.427% versus main; that benchmark forces eager parsing and does not use
this shortcut. R1 has not received full performance matrices or acceptance.

A local stack-location sample places 801 of 1,454 main-thread samples under the
new proof. The next candidate should combine proof and number normalization,
validate UTF-8 once, and compare fixed-width separators directly. No resulting
speedup is claimed yet. Source-string reuse additionally requires shared ownership
(`StringHeader.refcount == 0`), a complete source span and borrowed normalized output.
The standard record fixtures contain spellings such as `0.0`, so their normalization
produces owned output: whole-source reuse would not accelerate those rows.

## R2 combined traversal: still parked

R2 (`c812afd4841e79970d298f7b4ff9b97c9d8888ab`) combines proof and number
normalization, validates UTF-8 once, compares separators directly and derives
string ends from adjacent token offsets. It passes 298 JSON tests and all 162
compiled candidate comparisons, including deep and wide fallback cases.
[Validation and moving-GC witnesses](results/lazy-canonical-r2-validation/README.md).

Its [quiet focused replay](results/quiet-lazy-canonical-r2-main-focus-r9/README.md)
reduces the recorded record-roundtrip slowdowns to 28–36% versus main, compared
with 87–101% in the separate R1 window. It remains 50–65% slower than the current
PR build and is not accepted. Eager heterogeneous stringify remains +0.434%
versus main; the 13 KiB roundtrip has a 624 KiB median peak RSS increase.

The fresh local profile points to the scalar quote/backslash tail loop: the
prominent +1388/+1400 offsets are a byte load and character comparisons, not a
key-table lookup. The next bounded experiment should apply the existing padded
word scanner to short lazy-string bodies/tails while preserving the general
parser scanners. Validate every byte position and guard-page boundary before
measuring. Native output allocation/copying is a later target; no new output
ownership, global cache or GC-policy change has been implemented.
