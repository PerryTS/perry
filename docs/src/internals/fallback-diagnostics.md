# Generic fallback diagnostics

`PERRY_FALLBACK_DIAG=1 perry compile app.ts -o app` writes
`app.fallback.json` beside the executable. The opt-in compiler report names
emitted generic property reads, stores, computed accesses, presence checks,
method/call dispatch and birth/transition helpers. It records the helper's
compiler decision location, receiver facts and TOP origin, refused field-proof
predicates, selected guard, property key, generated function and block. An
aborted analysis is explicit, with its published work/pop counts; a missing
binding proof records that cause when the proof consumer actually rejects it.

Source byte offsets already retained by HIR resolve to source lines and UTF-8
byte columns. Nodes without their own offsets use the nearest enclosing
spanned expression when available, with `approximate: true`; otherwise the
function symbol is the location. An approximate location is never an exact
assignment or computed-access source span.

LLVM 22 call-site pseudo-probes provide the native labels. Diagnostic calls
carry `nomerge`, and backend tail merging is disabled in diagnostic images
because LLVM 22 statepoint lowering does not retain that machine-call flag.
These choices preserve distinct decision labels. A late operand fixup may follow the label; the ledger follows only
the same straight-line sequence to its first call and verifies the helper. The sidecar contains their
encoded sections and the linked symbol addresses, so tools can join call-site
labels to the measured executable's PCs, including duplicated/inlined sites.
`text_sha256` binds the evidence to the executable's `.text` section. Probe
sections, descriptors and synthetic DWARF are removed from the executable before its normal
strip step. The generated program contains no diagnostic state, counters,
validity bits, probe instructions or lookup table.

Object, completed-build and link caches are bypassed while diagnostics are on:
a warm cache must not omit records or preserve an already-removed probe table.
The unset and explicit `PERRY_FALLBACK_DIAG=0` paths produce the ordinary IR and
retain their existing cache identity. Runtime archives are unchanged.

The extraction currently requires `llvm-objcopy` or `objcopy`; the PC decoder
is validated on little-endian Linux ELF64 with LLVM 22. If the target cannot
emit probe labels or the labels cannot be extracted, the opt-in compile fails
instead of publishing an incomplete mapping as successful.

The diagnostic image must pass the instruction-count perturbation gate before
its profile is used. The off image carries none of these attributes.

Run the targeted acceptance check after a normal `cargo build-perry` build:

```sh
PERRY_RUNTIME_DIR=target/release python3 scripts/DG-fallback-gap.py target/release/perry
```

It checks Node parity, sidecar generation, warm-cache regeneration, removal of
probe tables, source-location accuracy labels, and whole-binary identity with
diagnostics off. Before using a diagnostic binary for profiling, compare its
retired user instructions against the default binary; pseudo-probes must not
materially perturb the measured lowering. The scoreboard ledger decodes labels
and keeps missing/ambiguous PC joins explicitly as `no record`.
