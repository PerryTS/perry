import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<unknown>();
const objectStore = { marker: "object" };
for (const [label, value] of [
  ["undefined", undefined],
  ["null", null],
  ["false", false],
  ["zero", 0],
  ["empty", ""],
  ["object", objectStore],
] as const) {
  const returned = storage.enterWith(value);
  console.log(
    `${label} enterWith:`,
    returned === undefined,
    Object.is(storage.getStore(), value),
  );
  await Promise.resolve();
  console.log(`${label} continuation:`, Object.is(storage.getStore(), value));
  storage.disable();
  console.log(`${label} disabled:`, storage.getStore() === undefined);
}
