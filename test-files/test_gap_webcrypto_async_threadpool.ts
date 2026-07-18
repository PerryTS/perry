// Node runs WebCrypto `subtle.digest` and async `crypto.randomBytes(cb)` on the
// libuv threadpool, so `await`ing them observably yields a macrotask — they do
// NOT resolve synchronously. Perry resolved them synchronously, which let an
// awaiting caller (e.g. Auth.js hashing a CSRF token in a Next.js Server
// Component) continue an event-loop iteration early and reorder streamed RSC
// output. The exact macrotask-hop count is threadpool/context dependent, so this
// pins the observable contract: async crypto crosses ≥1 macrotask, a sync hash
// crosses none, and values are unchanged.
import { webcrypto, randomBytes, createHash } from "crypto";
import { promisify } from "util";

let s = 0;
let run = true;
function metro(): void {
  if (!run) return;
  s++;
  setImmediate(metro);
}
metro();

async function crossesMacrotask(fn: () => Promise<unknown>): Promise<boolean> {
  const a = s;
  await fn();
  return s - a > 0;
}

async function main(): Promise<void> {
  console.log(
    "subtle.digest crosses macrotask:",
    await crossesMacrotask(() =>
      webcrypto.subtle.digest("SHA-256", new Uint8Array([1, 2, 3])),
    ),
  );
  console.log(
    "randomBytes(cb) crosses macrotask:",
    await crossesMacrotask(() => promisify(randomBytes)(16)),
  );

  // A synchronous hash stays synchronous.
  const a2 = s;
  createHash("sha256").update("x").digest("hex");
  console.log("createHash sync crosses macrotask:", s - a2 > 0);

  // Values are unchanged by the deferral.
  const d = await webcrypto.subtle.digest(
    "SHA-256",
    new TextEncoder().encode("hello"),
  );
  console.log("sha256(hello):", Buffer.from(d).toString("hex"));
  const rb = await promisify(randomBytes)(8);
  console.log("randomBytes length:", rb.length);

  run = false;
}

main();
