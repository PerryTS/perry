import { AsyncLocalStorage } from "node:async_hooks";
const storage = new AsyncLocalStorage<{ value: number }>();
async function main() {
  await storage.run({ value: 37 }, async () => {
    let resolve!: (value: number) => void;
    const promise = new Promise<number>(r => { resolve = r; });
    for (let i = 0; i < 30000; i++) {
      const pressure = { i, data: [i, i + 1] };
      if (pressure.i < 0) console.log(pressure);
    }
    resolve(11);
    console.log(await promise, storage.getStore()!.value);
    await Promise.resolve(12);
    console.log(storage.getStore()!.value);
  });
  console.log(storage.getStore() === undefined);
}
main();
