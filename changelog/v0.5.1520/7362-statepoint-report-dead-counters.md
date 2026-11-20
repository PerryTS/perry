### Fixed

**`--statepoint-report`: four counters that no code could ever write, and a test asserting one of them was zero.**

`FunctionRecord` declared `plain_stack_maps`, `stack_map_operands`,
`statepoint_fallbacks` and `fallbacks_by_callee`. All four were summed into the
totals and rendered into both the text and JSON reports. **None had a writer.**
The mutator API is `note_call` / `note_skipped` / `note_statepoint`, and none of
them touches those fields; `git log -S note_fallback` finds nothing, so they
were never populated — not orphaned when the plain-map bridge was deleted, but
dead from the start.

The report therefore printed `0 statepoint parser fallback(s)` as reassurance
that no root had been recorded in an unrecoverable location, a unit test
asserted that zero, and the comment above the assert said the structural zero
"is the point". A counter that cannot be non-zero is not evidence — it is
CLAUDE.md's fourth failure mode with the subject removed outright. The real
fail-closed guarantee is in `gc_map.rs`, which returns `Err` on an unparseable
or uncompactable map, so a fallback fails the *build*.

Replaced by `every_rendered_counter_has_a_writer`, which drives a record through
every mutator and asserts no scalar in the rendered totals is zero. It caught a
live field (`calls_without_live_roots`) on its first run, because the fixture
only made calls that had live roots — so the invariant has teeth.

Also cleaned up in the same sweep, all verified dead rather than assumed:

- `PERRY_STATEPOINTS` is **never read anywhere**. The empty-report diagnostic
  told users to set it (`Enable PERRY_STATEPOINTS=1`), which is a dead end:
  following the instruction produces the same empty report forever. It now names
  `PERRY_RS4GC=1` and the cache as the two real causes. Removed from the
  object-cache key list too — that test hashes the environment, so it passed for
  a name nothing reads and could never have flagged the drift.
- `declare void @llvm.experimental.stackmap` was emitted into every module and
  never called; removing it collapses two adjacent identical
  `native_stack_roots_enabled()` blocks into one.
- `compact_and_assemble` recomputed a `ptr64` predicate it never read, which
  read like a width guard that had been defeated. The emitter's own `ptr64` is
  the live one.
- `stack_maps.rs`'s module doc described a "research backend", two competing
  "prototypes" and a macOS-only implementation. It is the only backend, the
  plain-map lowering is deleted, and it supports Apple/Linux/Windows across
  aarch64 and x86-64.
