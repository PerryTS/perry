// #10523: every v4 UUID used to cost its own `getrandom` syscall. perry-uuid
// now serves `randomUUID()` from a per-thread cache refilled once per 128
// UUIDs (Node's batching), `{ disableEntropyCache: true }` bypasses it, and the
// global Web Crypto thunk calls the stdlib generator directly instead of
// re-dispatching `randomUUID` by name. Draw enough UUIDs through each entry
// point to cross several refills, interleaved so a path that consumed the pool
// out of step would repeat or skip bytes, and check every one is well formed
// and unique.
import { randomUUID as nodeRandomUUID } from "node:crypto";
import * as nodeCrypto from "node:crypto";

const re = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const hoisted = crypto.randomUUID.bind(crypto);
const extracted = crypto.randomUUID;

const paths: Record<string, () => string> = {
  "node:crypto named": () => nodeRandomUUID(),
  "node:crypto namespace": () => nodeCrypto.randomUUID(),
  "globalThis.crypto": () => crypto.randomUUID(),
  "bound": () => hoisted(),
  "extracted.call": () => extracted.call(crypto),
  "disableEntropyCache": () => nodeRandomUUID({ disableEntropyCache: true }),
  "disableEntropyCache false": () => nodeRandomUUID({ disableEntropyCache: false }),
};

const all = new Set<string>();
const counts: Record<string, number> = {};
let malformed = 0;
const names = Object.keys(paths);
for (let i = 0; i < 700; i++) {
  for (const name of names) {
    const id = paths[name]();
    if (typeof id !== "string" || !re.test(id)) malformed++;
    all.add(id);
    counts[name] = (counts[name] || 0) + 1;
  }
}
for (const name of names) console.log(`${name}: ${counts[name]}`);
console.log("total:", 700 * names.length);
console.log("unique:", all.size);
console.log("malformed:", malformed);

// A single path alone, straddling exactly one refill boundary each way.
const run = new Set<string>();
for (let i = 0; i < 128 * 4 + 1; i++) run.add(crypto.randomUUID());
console.log("single-path unique:", run.size);
