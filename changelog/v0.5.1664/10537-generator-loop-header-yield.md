### Fixed

- Generators now suspend on a `yield` in a loop header at any nesting depth
  (#10419). A `yield` in a `while`/`do…while` condition or a `for`
  condition/update was split into resume states only when the loop was a direct
  statement of the generator body (#5933). Inside `if`/`else`, `try`/`catch`/
  `finally`, a `switch` case, a label, or another loop, the residual yield never
  suspended: `[...g()]` was empty, a sent-value loop such as
  `while ((v = yield n) !== "stop")` never terminated, and `.return()`/`.throw()`
  found a finished generator. Minifiers emit exactly this shape — lru-cache
  11.5.2's `dist/esm/node/index.min.js` iterators (`keys`, `values`, `entries`,
  `rkeys`, `forEach`, `for…of`, `dump`) returned nothing and `clear()` skipped
  disposing entries.

  Root cause: the linearizer only descends into a compound statement when
  `body_contains_yield` reports a suspend point, and that check inspected loop
  bodies but never loop headers, so the `if`/`try`/`switch`/label/loop around a
  header-yield loop was emitted inline and the loop's per-iteration header arms
  never ran. `body_contains_yield` now checks while/do-while conditions and for
  conditions/updates. Three header positions the #5933 arms never covered, even
  at top level, are fixed alongside: a for-init's own top-level yield
  (`for (let t = yield x; …)`, `for (yield x; …)`, and an async function's
  `for (let x = await p; …)` after the await→yield rewrite) is hoisted ahead of
  the loop; a yielding update the header arm keeps in place (a `continue` inside
  `try`/`finally`) is linearized in the loop's update state; and in an
  `async function*` a header yield's operand is awaited like every other yield
  operand (a `yield promise` in a loop condition delivered the promise itself).

  Validation: `test_gap_10419_generator_loop_header_yield` (5 header positions ×
  12 containers, sent values, `return()`/`throw()` mid-loop, labeled continue,
  async generators with `for await`, plain-async for-init await) matches Node
  byte-for-byte and differs on the pre-fix compiler; `perry-transform` unit tests
  cover detection, the init hoist, residual-yield-free output and the async
  operand await. lru-cache 11.5.2's default entry now matches Node on the audit
  script (0 diff lines, was 14). Emitted LLVM IR for generators without header
  yields (and for top-level header yields) is byte-identical to before, so their
  instruction counts are unchanged.
