// `Readable.toWeb(nodeReadable)` should return a WHATWG ReadableStream.
// Perry's stub returns a fresh Duplex stand-in (data isn't propagated
// between Node and WHATWG universes yet), so the test asserts shape
// only — `typeof === "object"`. Regression cover for #1540.
import { Readable } from "node:stream";
const r = new Readable({ read() {} });
const w = Readable.toWeb(r);
console.log("typeof:", typeof w);
console.log("truthy:", !!w);
