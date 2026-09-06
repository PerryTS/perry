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
These diagnostics are not qualified CPU/RSS standings; the full matrix is running.

The two preceding parse-entry disassemblies both save and restore a large
register frame on the early container path. Equal instruction counts do not
prove equivalent execution cost. The single-output empty-parse review records
required ordinary-object flags and pressure/debt behavior; it is not an
implemented or measured change.

All-row parity and no-regression acceptance remain open.
