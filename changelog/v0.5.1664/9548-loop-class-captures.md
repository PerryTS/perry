### Fixed

- **Classes declared in a `for (let …)` body now retain that iteration's
  captured loop values.** Perry's shared-mutable class-capture lowering placed
  the loop binding in a one-element cell, but reused that cell across every
  iteration. Instances kept after the loop therefore all observed the final
  counter value (`3,3,3` instead of Node's `0,1,2`). The loop backedge now
  copies the current value into a fresh cell before evaluating the update,
  matching ECMAScript's per-iteration lexical environment while preserving
  the intentionally shared behavior of `for (var …)`.
