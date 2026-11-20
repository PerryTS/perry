import { readFileSync } from "node:fs";
import { AsyncLocalStorage } from "node:async_hooks";
import { createComponentHttpServer, closeComponentHttpServer } from "perry/component";
import { load } from "./generated/bindings";
import type { Component } from "./generated/bindings";
declare function gc(): void;

function check(ok: boolean, message: string): void { if (!ok) throw new Error(message); }
const bytes = readFileSync(process.argv[2]);
const tls = process.argv[3] === "tls";
const context = new AsyncLocalStorage<{ label: string }>();
function instance(): Component { return load(bytes, {}, { wasi: {}, fuel: 1000000 }); }
function listener() {
  return createComponentHttpServer({ address: "127.0.0.1:0", maxConnections: 1,
    requestHeadTimeoutMs: 5000, responseHeadTimeoutMs: 5000, betweenBytesTimeoutMs: 5000,
    lingerTimeoutMs: 10,
    ...(tls ? { tls: { certificatePem: readFileSync(process.argv[4], "utf8"), privateKeyPem: readFileSync(process.argv[5], "utf8") } } : {}) });
}

async function main(): Promise<void> {
  const server = listener();
  const reusable = instance();
  async function request(component: Component, name: string, mode: number): Promise<void> {
    component.exports.configure(mode);
    const marker = { label: name };
    context.enterWith(marker);
    const pending = component.handleHttp(server);
    gc();
    // The external peer waits for this timer. A blocking accept would hang.
    await new Promise<void>(resolve => setTimeout(() => { gc(); resolve(); }, 5));
    console.log("READY " + name + " " + server.address);
    let failed = false;
    try { await pending; } catch (_) { failed = true; }
    check(failed === (mode !== 0), "request result " + name);
    check(context.getStore() === marker, "request context " + name);
    if (mode === 1 || mode === 2 || mode === 4) {
      let disposed = false;
      try { component.exports.configure(0); } catch (_) { disposed = true; }
      check(disposed, "terminal guest failure must retire its store " + name);
    }
    gc();
    console.log("RESULT " + name);
  }
  await request(reusable, "success", 0);
  await request(reusable, "explicit-error", 3);
  await request(reusable, "reused", 0);
  reusable.dispose();
  for (const mode of [1, 2, 4]) {
    const component = instance();
    await request(component, "retire-" + mode, mode);
    component.dispose();
  }
  closeComponentHttpServer(server);
  closeComponentHttpServer(server);
  for (const disposeComponent of [false, true]) {
    const component = instance();
    const temporary = listener();
    const pending = component.handleHttp(temporary);
    gc();
    if (disposeComponent) component.dispose();
    else closeComponentHttpServer(temporary);
    let rejected = false;
    try { await pending; } catch (_) { rejected = true; }
    check(rejected, "pending accept cancellation");
    closeComponentHttpServer(temporary);
    component.dispose();
  }
  console.log("PASS component HTTP: streaming responses, errors, reuse, retirement, cancellation, context");
}
main().catch(error => { console.error(error); process.exitCode = 1; });
