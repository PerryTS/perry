import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<{
  label: string;
  immediate?: ReturnType<typeof setImmediate>;
}>();

const results = await Promise.all(
  ["first", "second", "third"].map(
    (label) =>
      new Promise<string>((resolve) => {
        storage.run({ label }, () => {
          const immediate = setImmediate(() => {
            const store = storage.getStore();
            const matches = store?.immediate === immediate;
            clearImmediate(immediate);
            resolve(`${store?.label}:${matches}:${immediate.constructor.name}`);
          });
          storage.getStore()!.immediate = immediate;
        });
      }),
  ),
);

console.log("self-cleared immediate contexts:", results.join("|"));
console.log("self-cleared immediate outside:", String(storage.getStore()));
storage.disable();
