import { createHook, executionAsyncId } from "node:async_hooks";
import { webcrypto } from "node:crypto";

const { subtle } = webcrypto;
const aesKey = await subtle.generateKey(
  { name: "AES-GCM", length: 128 },
  false,
  ["encrypt"],
);
const hmacKey = await subtle.generateKey(
  { name: "HMAC", hash: "SHA-256" },
  false,
  ["sign", "verify"],
);
const data = new TextEncoder().encode("async hooks webcrypto");
const types = ["HASHREQUEST", "CIPHERREQUEST", "SIGNREQUEST"] as const;
type Type = (typeof types)[number];
type Entry = {
  id: number;
  trigger: number;
  before: number;
  after: number;
  destroy: number;
};
const entries = new Map<Type, Entry[]>();
const byId = new Map<number, Entry>();
const parents = new Map<Type, number[]>();
const hook = createHook({
  init(asyncId, type, triggerAsyncId) {
    if (!types.includes(type as Type)) return;
    const entry = {
      id: asyncId,
      trigger: triggerAsyncId,
      before: 0,
      after: 0,
      destroy: 0,
    };
    const selected = entries.get(type as Type) || [];
    selected.push(entry);
    entries.set(type as Type, selected);
    byId.set(asyncId, entry);
  },
  before(asyncId) {
    const entry = byId.get(asyncId);
    if (entry) entry.before++;
  },
  after(asyncId) {
    const entry = byId.get(asyncId);
    if (entry) entry.after++;
  },
  destroy(asyncId) {
    const entry = byId.get(asyncId);
    if (entry) entry.destroy++;
  },
}).enable();

function remember(type: Type) {
  const selected = parents.get(type) || [];
  selected.push(executionAsyncId());
  parents.set(type, selected);
}
remember("HASHREQUEST");
await subtle.digest("SHA-256", data);
remember("CIPHERREQUEST");
await subtle.encrypt({ name: "AES-GCM", iv: new Uint8Array(12) }, aesKey, data);
remember("SIGNREQUEST");
const signature = await subtle.sign("HMAC", hmacKey, data);
remember("SIGNREQUEST");
const verified = await subtle.verify("HMAC", hmacKey, signature, data);
await new Promise<void>((resolve) => setImmediate(resolve));
await new Promise<void>((resolve) => setImmediate(resolve));
hook.disable();

console.log("webcrypto verified:", verified);
for (const type of types) {
  const selected = entries.get(type) || [];
  const expectedParents = parents.get(type) || [];
  console.log(
    `${type} lifecycle:`,
    selected.length,
    selected.length === expectedParents.length &&
      selected.every(
        (entry, index) => entry.trigger === expectedParents[index],
      ),
    selected
      .map((entry) => `${entry.before}/${entry.after}/${entry.destroy}`)
      .join(","),
  );
}
