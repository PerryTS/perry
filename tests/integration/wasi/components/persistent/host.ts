import { readFileSync } from "node:fs";
import { AsyncLocalStorage } from "node:async_hooks";
import { load } from "./generated/bindings";
declare function gc(): void;
function check(ok: boolean, message: string): void { if (!ok) throw new Error(message); }
const context = new AsyncLocalStorage<{ id: bigint }>();
const gates = new Map<bigint, () => void>();
let imports = 0;
const component = load(readFileSync(process.argv[2]), {
  "test:driver/clock": {
    "wait-for": async (id: bigint): Promise<void> => {
      imports++;
      check(context.getStore()?.id === id, "origin context before import await");
      gc();
      await new Promise<void>(resolve => gates.set(id, resolve));
      gc();
      check(context.getStore()?.id === id, "origin context after import await");
    },
  },
});
async function entered(count: number): Promise<void> {
  for (let attempt = 0; attempt < 100 && gates.size < count; attempt++) {
    await new Promise<void>(resolve => setTimeout(resolve, 1));
  }
  check(gates.size === count, "concurrent native imports actually suspended");
}
async function main(): Promise<void> {
  let firstDone = false;
  context.enterWith({ id: 1n });
  const first = component.exports.run(1n);
  first.then(() => { firstDone = true; });
  context.enterWith({ id: 2n });
  const second = component.exports.run(2n);
  check(first instanceof Promise && second instanceof Promise, "separate public Promises");
  await entered(2);
  gc();
  gates.get(2n)!();
  check(await second === 2n, "second call completes before first");
  check(!firstDone, "individual completion does not wait for global quiescence");
  gates.get(1n)!();
  check(await first === 1n, "first call preserves its result");
  gates.clear();
  context.enterWith({ id: 3n });
  const third = component.exports.run(3n);
  await entered(1);
  gates.get(3n)!();
  check(await third === 3n, "persistent Store is reusable");
  check(imports === 3, "all native imports executed");
  component.dispose();
  console.log("PASS persistent components: overlap, reverse completion, context, reuse, GC");
}
main().catch(error => { console.error(error); process.exitCode = 1; });
