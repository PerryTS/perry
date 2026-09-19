### Testing

- Lock commander's `_displayError` indirection in the gap suite: an object-property
  function (`outputError(str, write)`) invoking a second object-property function
  handed to it as a parameter (`writeErr`). #10711 reports that Perry silently drops
  that inner call and loses commander's error text; it does not reproduce. The
  reporter's own isolated repro matches Node 26.5.1 on current `main` (v0.5.1598), on
  the `main` commit their branch forks from (`8df83f8c`), and on their actual tree
  (PR #10712 over #10699) — each a full `-p perry -p perry-runtime-static
  -p perry-stdlib-static` build with `PERRY_RUNTIME_DIR` pinned, so no arm could have
  linked a stale archive. Real commander 14.0.3 compiled from source through
  `perry.compilePackages` is byte-identical to Node across the whole surface the issue
  names — `--help`, `--version`, missing required argument, unknown option, unknown
  command and `program.error()` — under the default output configuration and under a
  `configureOutput()` override, as are 32 further spellings of the same indirection
  (method shorthand, class field, spread, nested receiver, cross-object writer,
  computed key, getter, `Object.create` chain, `Object.freeze`, destructuring, three
  levels, async caller, nested closure, loop, and the shape inside a CommonJS module).

  The fixture is therefore a regression lock, not a fix, and it passes on unfixed
  `main`. The shape still earns a gate: #10689 — an inherited property read folding to
  the constant `undefined` on a scalar-replaced object — landed one commit before
  #10711 was filed, is the same family, and was silent in the same way, and nothing in
  `test-files/` covered this indirection.

  Two cases keep it from passing vacuously. One traces `before` / `typeof write` /
  `after` around the inner call so that "the outer body ran and the inner call
  evaporated" cannot read as a pass. The other omits the writer and asserts a
  `TypeError`, because a missing callee being loud is the property that keeps this bug
  class from presenting as a plausible wrong answer rather than a crash. Every writer
  sinks to stdout: the parity harness merges stdout and stderr into one compared
  stream, so a fixture using both would race on the interleaving.
