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
  const returned = storage.run(value, () => {
    console.log(label, "inside type:", typeof storage.getStore());
    console.log(label, "identity:", Object.is(storage.getStore(), value));
    return label;
  });
  console.log(label, "return:", returned);
  console.log(label, "restored:", String(storage.getStore()));
}
