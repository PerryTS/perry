### Fixed

- **`typeof` no longer intermittently answers `"function"` for an `Error`**
  (#10956). This is what kept the OpenCode TUI blank: its lock helper narrows
  a caught value with `typeof err !== "object"`, so an fs `EEXIST` it was
  written to swallow and retry was rethrown instead, and surfaced as
  `Unexpected server error`.

  **Root cause.** `typeof` decided "function" from four bytes alone: a
  `CLOSURE_MAGIC` ("CLOS") read at `CLOSURE_TYPE_TAG_OFFSET` (12 on 64-bit).
  In an `ErrorHeader` those four bytes are the struct padding between `flags`
  and `message`, and `alloc_error` wrote every field but never the padding.
  Arena slots are recycled without zeroing (bump reuse after a reset, and the
  exact-fit free list), and a 7-capture closure is a 72-byte payload, exactly
  the size of an `ErrorHeader`. An Error born in a slot a dead closure had
  vacated therefore kept that closure's magic. The object was otherwise
  well-formed (own `code`, correct prototype), which matches the report. It was
  intermittent because it depends on which slot the allocator hands out.

  `closure::is_closure_ptr` already refused a coincidental magic in an arena
  cell whose GC header is not `GC_TYPE_CLOSURE`, and its comment names this
  exact Error-padding case. The `typeof` classifier
  (`builtins/arithmetic.rs::classify_value_typeof`) never went through it; it
  kept its own bare read.

  **Fix.**
  - `classify_value_typeof` asks `is_closure_ptr`, so `typeof` agrees with
    every other closure test. That covers any arena cell with data at that
    offset, not just Errors: element 0's high word of an array
    (`typeof [15937034497556480]` answered `"function"` deterministically) and
    bytes 4..8 of a `Buffer`.
  - `alloc_error` zeroes the whole cell before it writes the fields, so a
    recycled slot cannot leak a stale tag into the padding. This also covers
    the other bare-magic probes (JSON's function filter, validators, and so on)
    for Errors.

  **Not covered.** A cell outside every arena (`HeapGeneration::Unknown`, e.g.
  a large `gc_malloc`'d object) still falls back to the exact-magic test in
  `is_closure_ptr`. Headerless static closures live there too, and telling the
  two apart would put a registry lookup on a hot path.

  **Tests.**
  - `error::header_unification_tests::an_error_born_in_a_dead_closure_slot_does_not_inherit_its_magic`
    recycles real dead 7-capture closures through the free list. It asserts the
    Error really lands in one of their slots, so it cannot pass on fresh
    memory, and then checks that the padding word is zero. Before the fix it
    read `0x434C4F53`.
  - `builtins::arithmetic::…::typeof_ignores_closure_magic_in_a_non_closure_cell`
    checks an Error with a planted tag, an array whose first element spells the
    tag, and a real closure (which is still `"function"`).
  - `test-files/test_gap_typeof_closure_magic_word.ts` is the Node-parity
    version.

  The inline `header_unification_tests` module moved to
  `error_header_tests.rs` without changes, to keep `error.rs` under the
  2,000-line cap.
