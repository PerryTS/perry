Depth preflight skips inputs whose byte length cannot exceed the nesting limit.
Every nesting level needs an opening byte; this proof holds even for malformed
input. The real parser still validates all syntax. Boundary tests retain the
first over-limit opening and the previous quoted-bracket handling.

Primitive-field preflight now uses a direct field borrow only for wide inline
objects. A successful preflight skips the general closure scan and emits with
the existing key order and scalar encoders. Small objects retain the previous
closure scan. Descriptors, class instances, overflow and complex fields retain
the generic walker. No GC production policy, suppression or scheduling changes.

139 JSON runtime tests, 33 compiled Node comparisons and 24 moving-GC runs
pass. All 43 source hashes match the built release. Local wide-object stringify
instructions fall another 9.87% against the primitive-object release, while
small-record instructions are essentially unchanged. Heterogeneous stringify
instructions increase 1.02%, removing the preceding small primitive-leaf benefit.
These instruction counters are diagnostics; the qualified CPU/RSS results follow.

The two preceding parse-entry disassemblies both save and restore a large
register frame on the early container path. Equal instruction counts do not
prove equivalent execution cost. The single-output empty-parse review records
required ordinary-object flags and pressure/debt behavior; it is not an
implemented or measured change.

All-row parity and no-regression acceptance remain open.

The complete full matrix is qualified: 152 output verifications, 456 timing
trials and 432 memory trials, with 31 clean external observations. A separate
546-trial paired comparison covers 39 cases against the primitive-object and
empty-object releases, with 39 clean observations. Both windows pass their
load and competing-process gates.

Against primitive-object, paired small-record parse improves 9.99%, tiny-object
parse 4.57%, empty-object parse 2.17%, 1 KB object parse 1.07%, and wide-object
stringify 7.15% (1824.23 to 1693.89 microseconds). Against empty-object, small
record parse improves 9.59%, tiny-object parse 2.46%, and wide-object stringify
31.69%. The full matrix retains roughly 71 MiB peak RSS for wide stringify.

Regressions remain against primitive-object: 1/8/20 MB record-object parse is
1.78%/2.13%/2.32% slower, 20 MB record-array parse is 2.80% slower, heterogeneous
stringify is 1.60% slower, 1 KB object stringify is 0.39% slower, and 8 MB record
object stringify is 0.41% slower, with separated paired ranges. Small-record
stringify is unchanged versus primitive-object but still 1.68% slower than
empty-object. Inline-string parse is still 2.03% slower than empty-object.
The apparent long-string stringify full-matrix slowdown does not reproduce.

The target inventory remains 11/38 CPU, 58/74 peak RSS and 28/36 retained RSS.
This is a development checkpoint, not all-row or no-regression acceptance.
All 43 source hashes match 3ffe5690410f7a8ea110c3f9e356ebb8eb7933b1. Full rows,
raw trials and paired ranges remain in this directory.
