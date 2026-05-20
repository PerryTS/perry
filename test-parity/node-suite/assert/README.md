# node:assert granular parity suite

Focused Node.js parity coverage for `node:assert` and `node:assert/strict`. Cases are small and deterministic, adapted from Node/Deno assert behavior into Perry's TypeScript parity runner style.

## Known gaps

The following behaviors are not yet wired up in Perry's `node:assert` runtime and therefore have no parity test in this suite:

- `assert.throws`, `assert.doesNotThrow`, `assert.rejects`, `assert.doesNotReject` — invoking a JS closure from a Rust runtime helper with `try`/`catch` semantics requires setjmp to be installed in the Rust frame, which is not currently exposed. A future fix can synthesize the try/catch in codegen instead. Calls today fail at compile time with `not implemented in Perry`.
- `assert.CallTracker.prototype.calls` and friends — only the constructor shape is exposed; instance methods (`calls`, `verify`, `report`) are missing.
- `assert.deepStrictEqual` for nested arrays, typed arrays (`Uint8Array` etc.), and null-prototype objects — comparison crashes or diverges from Node. `Date` and `RegExp` equality work.
