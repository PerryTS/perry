// #11453: a long-running process that creates native handles per operation
// (a digest per request, an HMAC, a StringDecoder) used to leak every one of
// them: the payload stayed registered forever and its id was never returned
// to the shared 262k-id band, so the process panicked with
// `common native handle registration exhausted: IdExhausted` after ~200k
// operations. Handles nothing references are now reclaimed by the collector
// and their ids recycled. A handle the program still holds keeps its id and
// its state: a stale reference never resolves to a newer object.
import * as crypto from "node:crypto";
import { StringDecoder } from "node:string_decoder";

const held = crypto.createHash("sha256");
held.update("the-first-and-only-update");
const heldDigest = held.digest("hex");
const heldHmac = crypto.createHmac("sha256", "k").update("hmac-payload-for-the-held-handle");
const heldDecoder = new StringDecoder("utf8");
const partial = heldDecoder.write(Buffer.from([0xe2, 0x82]));

const N = 1_100_000;
let sum = 0;
for (let i = 0; i < N; i++) {
  const h = crypto.createHash("sha1").update("request-path-/api/items/" + (i & 7)).digest("hex");
  sum += h.charCodeAt(i & 31);
  if ((i & 15) === 0) {
    sum += crypto.createHmac("sha256", "key").update("session-cookie-value-" + (i & 3)).digest("base64").length;
  }
  if ((i & 63) === 0) {
    const d = new StringDecoder("utf8");
    sum += d.write(Buffer.from("hé")).length + d.end().length;
  }
}
console.log("cycles", N, "sum", sum);

// The finalized hash is still the same object: using it again throws.
try {
  held.update("an-update-after-digest");
  console.log("stale update: no throw");
} catch (e: any) {
  console.log("stale update:", e.code);
}
console.log("held digest", heldDigest.slice(0, 16));
// The held HMAC and decoder kept their own state across a million
// allocations: neither id was handed to another object.
console.log("held hmac", heldHmac.digest("hex").slice(0, 16));
console.log("held decoder", JSON.stringify(partial + heldDecoder.write(Buffer.from([0xac]))));
