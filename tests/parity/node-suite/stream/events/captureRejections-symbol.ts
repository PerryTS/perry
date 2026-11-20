import { EventEmitter, captureRejectionSymbol } from "node:events";
import { Readable } from "node:stream";
// EventEmitter exposes Symbol.for("nodejs.rejection") as
// captureRejectionSymbol. Streams inherit EE, so the same symbol exists.
console.log("symbol is symbol:", typeof captureRejectionSymbol === "symbol");
const r = new Readable({ read() { this.push(null); } });
// streams don't override the symbol — assertion: they're EE-shaped
console.log("instanceof EventEmitter:", r instanceof EventEmitter);
console.log("captureRejections accessor exists:", typeof (r as any).captureRejections);
