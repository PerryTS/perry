# node:assert granular parity suite

Focused Node.js parity coverage for `node:assert` and `node:assert/strict`. Cases are small and deterministic, adapted from Node/Deno assert behavior into Perry's TypeScript parity runner style.

## Known gaps

The following behaviors are not yet wired up in Perry's `node:assert` runtime and therefore have no parity test in this suite:

- `assert.CallTracker.prototype.calls` and friends — only the constructor shape is exposed; instance methods (`calls`, `verify`, `report`) are missing.
- `assert.deepStrictEqual` for nested arrays, typed arrays (`Uint8Array` etc.), and null-prototype objects — comparison crashes or diverges from Node. `Date` and `RegExp` equality work.
