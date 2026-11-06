import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

function makeThenable(label: string) {
  return {
    then(resolve: (value: string) => void) {
      console.log(label, "then store:", storage.getStore());
      queueMicrotask(() => {
        console.log(label, "resolve store:", storage.getStore());
        resolve(`${label}-value`);
      });
    },
  };
}

const awaited = await storage.run("await-context", async () => {
  const value = await makeThenable("await");
  console.log("await continuation:", storage.getStore(), value);
  return value;
});
console.log("await result:", awaited);

const resolved = await storage.run("resolve-context", () =>
  Promise.resolve(makeThenable("resolve")).then((value) => {
    console.log("resolve continuation:", storage.getStore(), value);
    return value;
  }),
);
console.log("resolve result:", resolved);
console.log("thenable outside:", String(storage.getStore()));
