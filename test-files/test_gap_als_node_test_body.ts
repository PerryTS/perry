// The callback of an internal runtime completion runs in the AsyncLocalStorage
// context of the operation that started it (als.run -> operation -> callback).
import { AsyncLocalStorage } from "node:async_hooks";
const als = new AsyncLocalStorage<string>();

// The body exits before the reporter prints its (timing-dependent) summary.
import { test } from "node:test";
als.run("test", () => {
  test("t", () => { console.log("test body: " + als.getStore()); process.exit(0); });
});
