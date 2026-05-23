// `stream.compose(...streams)` chains a sequence of streams into a
// composite Duplex (data flows through them in order). Perry's stream
// stubs don't propagate data through chains yet, but the typeof
// result (composite is an object) needs to match so consumers that
// branch on `typeof compose(...) === "object"` don't fall through.
// Regression cover for #1539. Real composition is tracked separately.
import { compose, Readable, Writable } from "node:stream";
const out = compose(
  new Readable({ read() {} }),
  new Writable({ write(_a, _b, c) { c(); } }),
);
console.log("typeof:", typeof out);
