// Async crypto must settle after the current microtask checkpoint. A
// setImmediate counter races native completion in Node. Keep the checkpoint
// busy with a bounded microtask chain: native completion waits until it drains;
// an already-resolved promise or a short microtask-only implementation does not.
import { webcrypto, randomBytes, createHash } from "crypto";
import { promisify } from "util";

async function defersCompletion(fn: () => Promise<unknown>): Promise<boolean> {
  let sentinelRan = false;
  const observed = fn().then(() => sentinelRan);
  let remaining = 1000;
  function checkpointSentinel(): void {
    remaining--;
    if (remaining === 0) sentinelRan = true;
    else queueMicrotask(checkpointSentinel);
  }
  queueMicrotask(checkpointSentinel);
  return observed;
}

function threeMicrotasks(): Promise<void> {
  return new Promise<void>((resolve) => {
    queueMicrotask(() => {
      queueMicrotask(() => {
        queueMicrotask(() => resolve());
      });
    });
  });
}

function expectDeferred(deferred: boolean, operation: string): void {
  if (!deferred) throw new Error(operation + " resolved before the sentinel");
}

async function main(): Promise<void> {
  const resolved = await defersCompletion(() => Promise.resolve(undefined));
  if (resolved) throw new Error("resolved-promise negative control failed");
  console.log("resolved Promise defers completion:", resolved);

  const microtaskOnly = await defersCompletion(threeMicrotasks);
  if (microtaskOnly) throw new Error("three-microtask negative control failed");
  console.log("three microtasks defer completion:", microtaskOnly);

  const digest = await defersCompletion(() =>
    webcrypto.subtle.digest("SHA-256", new Uint8Array([1, 2, 3])),
  );
  expectDeferred(digest, "subtle.digest");
  console.log("subtle.digest defers completion:", digest);

  const random = await defersCompletion(() => promisify(randomBytes)(16));
  expectDeferred(random, "randomBytes(cb)");
  console.log("randomBytes(cb) defers completion:", random);

  // A synchronous hash must finish before a queued microtask can run.
  let syncSentinelRan = false;
  queueMicrotask(() => { syncSentinelRan = true; });
  createHash("sha256").update("x").digest("hex");
  if (syncSentinelRan) throw new Error("synchronous hash yielded");
  console.log("createHash sync yields to microtask:", syncSentinelRan);

  // Values are unchanged by the deferral.
  const d = await webcrypto.subtle.digest(
    "SHA-256", new TextEncoder().encode("hello"),
  );
  console.log("sha256(hello):", Buffer.from(d).toString("hex"));
  const rb = await promisify(randomBytes)(8);
  console.log("randomBytes length:", rb.length);
}

main();
