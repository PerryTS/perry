import {
  createHook,
  executionAsyncId,
  executionAsyncResource,
} from "node:async_hooks";
import { generateKey, generateKeyPair, hkdf, scrypt } from "node:crypto";

const types = [
  "DERIVEBITSREQUEST",
  "SCRYPTREQUEST",
  "KEYGENREQUEST",
  "KEYPAIRGENREQUEST",
] as const;
type Type = (typeof types)[number];
type Entry = {
  asyncId: number;
  triggerAsyncId: number;
  resource: object;
  before: number;
  after: number;
  destroy: number;
};
const entries = new Map<Type, Entry[]>();
const byId = new Map<number, Entry>();
const expectedParents = new Map<Type, number>();
const callbackChecks = new Map<Type, boolean>();
const hook = createHook({
  init(asyncId, type, triggerAsyncId, resource) {
    if (!types.includes(type as Type)) return;
    const entry = {
      asyncId,
      triggerAsyncId,
      resource,
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
  expectedParents.set(type, executionAsyncId());
}
function callbackMatches(type: Type) {
  const entry = entries.get(type)?.[0];
  callbackChecks.set(
    type,
    !!entry &&
      executionAsyncId() === entry.asyncId &&
      executionAsyncResource() === entry.resource,
  );
}

try {
  remember("DERIVEBITSREQUEST");
  await new Promise<void>((resolve, reject) => {
    hkdf("sha256", "key", "salt", "info", 8, (error) => {
      callbackMatches("DERIVEBITSREQUEST");
      error ? reject(error) : resolve();
    });
  });

  remember("SCRYPTREQUEST");
  await new Promise<void>((resolve, reject) => {
    scrypt("password", "salt", 16, (error) => {
      callbackMatches("SCRYPTREQUEST");
      error ? reject(error) : resolve();
    });
  });

  remember("KEYGENREQUEST");
  await new Promise<void>((resolve, reject) => {
    generateKey("hmac", { length: 128 }, (error) => {
      callbackMatches("KEYGENREQUEST");
      error ? reject(error) : resolve();
    });
  });

  remember("KEYPAIRGENREQUEST");
  await new Promise<void>((resolve, reject) => {
    generateKeyPair("ed25519", {}, (error) => {
      callbackMatches("KEYPAIRGENREQUEST");
      error ? reject(error) : resolve();
    });
  });

  await new Promise<void>((resolve) => setImmediate(resolve));
  await new Promise<void>((resolve) => setImmediate(resolve));
} finally {
  hook.disable();
}

for (const type of types) {
  const selected = entries.get(type) || [];
  console.log(
    `${type} crypto lifecycle:`,
    selected.length,
    selected.length === 1 &&
      selected.every(
        (entry) => entry.triggerAsyncId === expectedParents.get(type),
      ),
    callbackChecks.get(type) === true,
    selected
      .map((entry) => `${entry.before}/${entry.after}/${entry.destroy}`)
      .join(","),
  );
}
