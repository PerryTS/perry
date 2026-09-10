Optimize the ARM64 JSON nesting precheck by scanning opening-container bytes in 64-byte blocks. Preserve the initial 16-byte early-positive check and exact behavior for arbitrary input bytes. Add vector-boundary and out-of-slice poison coverage. JSON construction and GC scheduling continue through their existing paths. Keep empty-object allocation outside the shared parse entry so scalar and nonempty-container dispatch do not inherit its register frame. Keep the wide search tail in a separate function so the shared depth state machine retains a smaller instruction footprint. Reuse the native tape builder's nesting information for eligible arrays, retaining iterative materialization and guarded malformed-input fallback. Keep native tape construction outside the common parse entry.

Lazy stringify source admission:

Validate compact string spelling, duplicate keys and integer-key order before
copying an untouched lazy array's source. Noncanonical sources use ordinary
materialization and serialization; native admission scratch adds no GC roots.
This corrects inherited whitespace, escape, duplicate-key and enumeration bugs.
Perform source admission and number normalization in one tape walk, validate
UTF-8 once, and use adjacent token offsets to avoid repeating string-end scans.
Use bounded padded-word classification for four-to-seven-byte lazy-string tails,
while retaining the existing general parser and escaper scanner behavior.
Record a positive no-escape marker in the existing native key/string tape field
during syntax validation. Lazy source admission can then skip a second body
scan for those tokens, while unknown metadata retains the checked fallback.
Tape entry size, managed object construction and collector policy are unchanged.
