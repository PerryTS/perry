Fix early iterator close in synchronous generator for-of loops and direct generator array destructuring. Close after abrupt binding/body completion while preserving exception precedence, avoid close after a failed iterator step, and preserve unstarted-generator behavior.

Synchronous yield* now forwards return and throw, closes delegates without throw, and routes protocol failures through enclosing catch/finally blocks. Spread and Array.from are covered by Node parity tests and retain their existing behavior.

Outline close-on-throw and synchronous delegation protocol operations into shared runtime helpers. Cold synchronous abrupt resumes reuse the private next dispatcher. On Linux x86-64, iterator cleanup pads avoid per-iteration exception savepoints, respecting the nearest runtime callback trap. Other targets keep registered cleanup handlers.

Keep the native cleanup search in a separate archive member reached through a weak hook, so programs without iterator cleanup pads do not retain the exception personality or its decoder.

Use the existing iterator protocol HIR channel for outlined close calls so proven synchronous programs do not retain an unnecessary event loop. Callbacks that schedule work still keep the loop.

Resolve the native FDE lookup through the existing loader only on the cold preflight path. Avoiding its additional PLT import saves a whole ELF header page in small programs; the live-unwinder unit verifies the lookup.

Keep the cold unwinder name in a discardable read-only section so programs without iterator cleanup do not retain it or shift their hot runtime literals.
