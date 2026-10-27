import { readFileSync } from "node:fs";
import { AsyncLocalStorage } from "node:async_hooks";
import { createComponentHttpServer, closeComponentHttpServer } from "perry/component";
import { load } from "./generated/bindings";
declare function gc(): void;
function check(ok: boolean, message: string): void { if (!ok) throw new Error(message); }
const context = new AsyncLocalStorage<{ name: string }>();
const slow = { name: "slow" };
const fast = { name: "fast" };
let slowCalls = 0;
let fastCalls = 0;
function listener() {
  return createComponentHttpServer({ address: "127.0.0.1:0", maxConnections: 1,
    requestHeadTimeoutMs: 10000, responseHeadTimeoutMs: 10000, betweenBytesTimeoutMs: 10000,
    lingerTimeoutMs: 10,
    ...(process.argv[3] === "tls" ? { tls: { certificatePem: readFileSync(process.argv[4], "utf8"), privateKeyPem: readFileSync(process.argv[5], "utf8") } } : {}) });
}
async function main(): Promise<void> {
  const a = listener();
  const b = listener();
  const component = load(readFileSync(process.argv[2] + "/concurrent.wasm"), {
    before: async (): Promise<void> => {
      const expected = context.getStore();
      check(expected === slow || expected === fast, "each callback has its own request context");
      if (expected === slow) slowCalls++; else fastCalls++;
      gc();
      await new Promise<void>(resolve => setTimeout(() => { gc(); resolve(); }, 5));
      check(context.getStore() === expected, "request context survives suspension and moving GC");
      gc();
    },
  }, { wasi: {}, fuel: 1000000 });
  context.enterWith(slow);
  let firstDone = false;
  const first = component.handleHttp(a).then(() => {
    check(context.getStore() === slow, "slow completion context"); firstDone = true;
  });
  context.enterWith(fast);
  const second = component.handleHttp(b);
  gc();
  console.log("READY concurrent " + a.address + " " + b.address);
  await second;
  check(context.getStore() === fast, "fast completion context");
  check(!firstDone, "fast request completes while slow upload remains suspended");
  check(fastCalls === 2 && slowCalls > 0, "both requests entered real async imports");
  gc();
  console.log("FAST concurrent");
  await first;
  check(firstDone, "slow request eventually finishes");
  check(slowCalls === 2, "slow request completed both async imports");
  component.dispose(); closeComponentHttpServer(a); closeComponentHttpServer(b);
  gc();
  console.log("PASS concurrent P3 HTTP: independent task exit, native bodies, per-request ALS and GC");
}
main().catch(error => { console.error(error); process.exitCode = 1; });
