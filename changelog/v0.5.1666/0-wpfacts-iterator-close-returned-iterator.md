Whole-program facts: an iterator that `[Symbol.iterator]()` returns is the
value a destructuring or a `for…of` loop closes. Its `return` method ran in Node
(on `break`, an early `return`, a throw, or a destructuring that stops before
the end) while the facts claimed it never runs; the run-time claim check mode
reported it on twelve gap programs. The facts now count it as a body that runs.
