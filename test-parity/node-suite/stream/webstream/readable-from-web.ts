// `Readable.fromWeb(webStream)` should return a Node Readable
// wrapping the WHATWG ReadableStream. Perry's stub returns a fresh
// Duplex stand-in (the WHATWG-side data isn't pulled through yet);
// the test asserts shape only — `typeof === "object"`. Regression
// cover for #1540.
import { Readable } from "node:stream";
const web = new ReadableStream();
const r = Readable.fromWeb(web);
console.log("typeof:", typeof r);
console.log("truthy:", !!r);
