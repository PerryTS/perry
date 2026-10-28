import { readFileSync } from "node:fs";
import { AsyncLocalStorage } from "node:async_hooks";
import { load } from "./generated/bindings";
import type { HostResources } from "./generated/bindings";
declare function gc(): void;

function check(ok: boolean, message: string): void { if (!ok) throw new Error(message); }
const bytes = readFileSync(process.argv[2]);
const context = new AsyncLocalStorage<object>();
let now = 0n;
let calls = 0;
let mode = "success";
let expected: object = {};
let rejection: object = {};
let dropped = 0;
function instance() {
  let factories: HostResources;
  const component = load(bytes, resources => {
    factories = resources;
    return {
    "test:async-clock/clock@1.0.0": {
      now: () => { const value = now; now += 100n; return value; },
      "wait-for": async (nanos: bigint): Promise<void> => {
        calls++;
        check(nanos === 7n, "owned import argument");
        check(context.getStore() === expected, "import context before await");
        gc();
        if (mode === "cancel") await new Promise<void>(() => {});
        await new Promise<void>(resolve => setTimeout(resolve, 5));
        gc();
        check(context.getStore() === expected, "import context after await");
        if (mode === "reject") throw rejection;
      },
    },
    };
  });
  return {
    exports: component.exports,
    dispose: () => component.dispose(),
    item: () => factories["test:async-clock/clock@1.0.0"].item.create({ label: "retained payload" }, payload => {
      gc();
      check((payload as { label: string }).label === "retained payload", "destructor retains payload through GC");
      dropped++;
      if (mode === "reject") throw new Error("secondary destructor failure");
    }),
  };
}
async function main(): Promise<void> {
  const reusable = instance();
  check(typeof reusable.exports.ping() === "bigint", "sync exports work in an async world");
  for (let index = 0; index < 3; index++) {
    expected = { index };
    context.enterWith(expected);
    const item = reusable.item();
    const pending = reusable.exports.run(7n, item);
    check(pending instanceof Promise, "async exports return Promises");
    gc();
    check(await pending === 100n, "canonical async result");
    check(context.getStore() === expected, "caller context after await");
    check(dropped === index + 1, "canonical guest resource drop invokes JS destructor");
  }
  reusable.dispose();
  for (const action of ["reject", "cancel"]) {
    mode = action;
    expected = { action };
    rejection = { message: "original async rejection" };
    context.enterWith(expected);
    const component = instance();
    const before = calls;
    const pending = component.exports.run(7n, component.item());
    // Let the native invocation enter its suspended JS import before disposal.
    await new Promise<void>(resolve => setTimeout(resolve, 1));
    check(calls === before + 1, "native import actually suspended");
    if (action === "cancel") component.dispose();
    gc();
    let failed = false;
    try { await pending; }
    catch (error) {
      failed = true;
      if (action === "reject") check(error === rejection, "rejection identity survives cleanup");
    }
    check(failed, action + " rejects the call");
    let disposed = false;
    try { component.exports.ping(); } catch (_) { disposed = true; }
    check(disposed, "terminal async failure retires the store");
    component.dispose();
  }
  check(calls === 5, "all canonical async imports ran");
  check(dropped === 5, "guest-held payloads retire exactly once");
  console.log("PASS component async: suspension, reuse, rejection identity, cancellation, context, GC");
}
main().catch(error => { console.error(error); process.exitCode = 1; });
