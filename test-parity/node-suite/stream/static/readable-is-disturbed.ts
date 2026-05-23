// Stream static introspection helpers `Readable.isDisturbed(s)` /
// `Readable.isErrored(s)`. For a freshly-constructed, untouched
// stream both return `false`. Perry's stream stubs don't track
// state, so the stub also returns `false` — which is the correct
// answer for this shape of usage. Regression cover for #1534.
//
// Directional helpers `isReadable` / `isWritable` deliberately not
// asserted: Node's answer depends on direction (Readable returns
// `true` for isReadable + `null` for isWritable) and Perry's stub
// doesn't carry direction at runtime yet — they're tracked as a
// follow-up in #1534.
import { Readable } from "node:stream";
const r = new Readable({ read() {} });
console.log("isDisturbed:", Readable.isDisturbed(r));
console.log("isErrored:", Readable.isErrored(r));
