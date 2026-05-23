// `addAbortSignal(signal, stream)` wires the AbortSignal so aborting
// it destroys the stream — and returns the stream for chaining.
// Perry's stream stubs don't track destroy/abort yet, so the stub
// only matches Node on the identity-return shape (the common
// `r = addAbortSignal(c.signal, r)` chain pattern). Regression
// cover for #1541. Real abort-propagation is tracked separately.
import { addAbortSignal, Readable } from "node:stream";
const c = new AbortController();
const r = new Readable({ read() {} });
console.log("returns same:", addAbortSignal(c.signal, r) === r);
