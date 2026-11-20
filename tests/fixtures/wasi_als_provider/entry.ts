import { AsyncLocalStorage } from "node:async_hooks";
import { checkStore, readAfterAwait, storage } from "./context.ts";

// The native host loads this app after the separate runtime and stdlib images.
// Snapshot/bind and Promise restoration must all observe the provider's context.
export async function runWasiAlsProvider(): Promise<void> {
  checkStore(undefined);
  const snapshot = storage.run({ label: "snapshot" }, () => AsyncLocalStorage.snapshot());
  const bound = storage.run({ label: "bound" }, () =>
    AsyncLocalStorage.bind(() => checkStore("bound")),
  );
  storage.enterWith({ label: "caller" });
  const first = storage.run({ label: "first" }, async () => {
    checkStore("first");
    storage.exit(() => checkStore(undefined));
    checkStore("first");
    await readAfterAwait("first");
    bound();
    checkStore("first");
    await snapshot(async () => {
      checkStore("snapshot");
      await readAfterAwait("snapshot");
    });
    checkStore("first");
  });
  checkStore("caller");
  const second = storage.run({ label: "second" }, async () => {
    await readAfterAwait("second");
    checkStore("second");
  });
  checkStore("caller");
  await Promise.all([first, second]);
  checkStore("caller");
  storage.disable();
  checkStore(undefined);
  console.log("provider ALS: overlapping calls, snapshot, bind, exit and restoration passed");
}
