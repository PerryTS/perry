Before promoting the structural scanner, resolve the qualified full-matrix
null/string_a parse regressions and investigate the RSS increases. The native
`js_json_parse` entry still has 17 instructions and `parse_noncontainer` still
has 425 in both binaries; their alignment modulo 64 bytes is also unchanged.
The symbol addresses and some constant-page references moved. The affected
scalar inputs never execute the depth preflight. This is not proof that code
placement caused the slowdown.

The scalar decoder currently pays for a 144-byte native frame even on null.
A bounded small-scalar decoder or different outlining may reduce that work,
but must preserve whitespace, strict syntax, inline-string UTF-8 checks, and
the existing post-decode GC/cache service. No such runtime change is included.

The next stringify experiment will specialize the temporary plan for object keys.
Current key plans reuse the 40-byte `Piece` enum, including space for a 32-byte
number buffer that keys never use. The record path reserves eight such entries;
the flat-object path reserves four. Value and array-element plans stay as they are.

The ABI probe in `key-plan-abi/` changes the intended design: on this exact ARM64
Rust toolchain, `Option<Triple>` occupies 12 bytes but uses an indirect return
buffer. Packing quoted byte and UTF-16 counts into a `NonZeroU64` alongside a
`u32` escaped-source length occupies 16 bytes and returns in two registers.
The optimized LLVM signatures and assembly are preserved. This is evidence
about representation and calling convention, not a measured runtime speedup.

The proposed representation has no managed pointers. A zero source length means
unescaped; escaped strings necessarily contain at least one source byte. Plan
construction must preserve the existing overflow checks, null rejection,
non-ASCII synthetic SSO fallback, incomplete-tail handling, and escaped UTF-8
validation. The parent remains rooted before prototype initialization and final
output allocation. Keys must be rederived from that root after allocation.
No allocation, callbacks, or collection may occur during emission.

The current local sample has a separate startup segment; it is not included in
application hotspot reasoning. The application segment still contains key/value
planning, output copies and prototype checks. Sampling is diagnostic only, on
a different machine from the qualified CPU/RSS comparison. Earlier broad
`Piece` layout and reference-passing experiments regressed complete workloads;
therefore this narrower key-only design needs the full matrix before selection.

A further parse investigation is to avoid the separate depth preflight on paths
that already validate using an explicit stack. This needs a shared proof for all
parse entry points and the existing 500,000-level iterative budget. Simply
removing the guard would expose recursive validation/materialization to unsafe
stack depth. It is not part of the present change.

The separate diag30 window confirms the scalar slowdown at 20 million calls.
Its escaped-parse traces give equal collection counts and scan work, but one
candidate retains an extra conservative root and arena block. Another has
matching live bytes yet still higher process peak RSS. Trace native retention
and resident-page differences before attributing all of the RSS change to GC.
