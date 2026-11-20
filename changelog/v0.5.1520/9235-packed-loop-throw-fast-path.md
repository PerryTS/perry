### Performance

- **A `throw` inside a counted loop no longer forces the loop off its fast
  path.** `for (let i = 0; i < arr.length; i++) { if (bad) throw new Error(…);
  sum += arr[i]; }` is the ordinary shape of a validating loop, and the throw
  — even one that never fires — used to cost the whole packed-array
  specialization. Measured on a quiet machine over a 512-element `number[]`:

  | loop body | before | after | node |
  |---|---|---|---|
  | `throw new Error("bad")` | 7.99 ns/op | **0.95** | 7.70 |
  | `throw new Error("bad " + i)` | 7.99 | **0.95** | 7.75 |
  | `throw new Error()` | 7.99 | **0.95** | 7.75 |
  | `throw "bad " + i` | 7.99 | **0.95** | 7.74 |
  | `throw <pre-built value>` | 7.99 | **0.95** | 1.11 |

  An 8.1× improvement on the constructing forms, which are the common ones.
  Perry now runs every shape in that benchmark faster than node, including
  the loop with no `throw` in it at all (0.95 against 1.11).

  The specialization keeps a loop's accumulators in registers, so admitting a
  `throw` required writing them back on the unwind edge — an exception leaves
  through a landing pad rather than the loop's exit block, and a `catch` can
  read anything the loop wrote. Operands are admitted only when evaluating
  them cannot itself unwind: throwing a value coerces nothing, but building an
  `Error` message or concatenating a string can dispatch to a user `toString`
  or `valueOf`, and those can throw before the writeback runs.

  These landed in **v0.5.1519** (#9235, with #9230 and #9215). This fragment
  was written after that release was cut, so it appears here rather than in
  the notes for the release that carries the change.
