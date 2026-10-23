import { readFileSync } from "node:fs";
import { AsyncLocalStorage } from "node:async_hooks";
import { createComponentHttpServer, closeComponentHttpServer } from "perry/component";
import { load } from "./generated/bindings";
import type { Component } from "./generated/bindings";
declare function gc(): void;

function check(ok: boolean, message: string): void { if (!ok) throw new Error(message); }
const bytes = readFileSync(process.argv[2]);
const tls = process.argv[3] === "tls";
const port = Number(process.argv[6]);
const guestMoving = process.argv[7] === "moving";
const context = new AsyncLocalStorage<{ label: string }>();
// A collection after every 4 KiB deliberately spends substantial guest fuel.
function instance(network: any = {}, fuel = 100000000000): Component {
  return load(bytes, {}, { memoryBytes: 268435456, fuel,
    wasi: { inheritStderr: true, network, env: guestMoving ? {
      PERRY_GC_FORCE_EVACUATE: "1", PERRY_GC_VERIFY_EVACUATION: "1",
      PERRY_GC_DIAG: "1", PERRY_GC_INSTRUMENTS: "1"
    } : {} } });
}
function listener() {
  return createComponentHttpServer({ address: "127.0.0.1:0", maxConnections: 1,
    requestHeadTimeoutMs: 15000, responseHeadTimeoutMs: 15000, betweenBytesTimeoutMs: 15000,
    lingerTimeoutMs: 10,
    ...(tls ? { tls: { certificatePem: readFileSync(process.argv[4], "utf8"), privateKeyPem: readFileSync(process.argv[5], "utf8") } } : {}) });
}
function outgoing(): void {
  const remote = { address: "127.0.0.1", port };
  const wrongPort = port === 65535 ? 65534 : port + 1;
  for (const network of [ {}, { udp: true, addresses: [remote] }, { tcp: true },
    { tcp: true, addresses: [{ address: "127.0.0.2", port }] },
    { tcp: true, addresses: [{ address: "127.0.0.1", port: wrongPort }] } ]) {
    const component = instance(network);
    for (let i = 0; i < 2; i++) {
      gc();
      const result = component.exports.outgoing(port, new Uint8Array([1, 0, 255]));
      check(result.tag === "err", "HTTP destination denial");
      component.exports.configure(0);
    }
    component.dispose();
  }
  // A large canonical argument must fit this budget before HTTP starts.
  // The former per-byte JS setter exhausted it while lifting the byte list.
  const bounded = instance({}, 1000000000);
  check(bounded.exports.outgoing(port, new Uint8Array(3145865)).tag === "err",
    "large byte-list conversion must reach the denied HTTP operation");
  bounded.dispose();
  console.log("DENIED 11");
  const component = instance({ tcp: true, addresses: [remote] });
  for (const length of [0, 7, 3145865, 17]) {
    const payload = new Uint8Array(length);
    for (let i = 0; i < length; i++) payload[i] = (i * 31 + 7) % 256;
    gc();
    const result = component.exports.outgoing(port, payload);
    check(result.tag === "ok", "outgoing result: " + (result.tag === "err" ? result.value : "ok"));
    if (result.tag === "ok") {
      gc();
      check(result.value.length === length, "outgoing response length");
      for (let i = 0; i < length; i++) check(result.value[i] === payload[i], "outgoing response bytes");
    }
    console.log("OUTGOING " + length);
  }
  component.dispose();
}
async function main(): Promise<void> {
  outgoing();
  const server = listener();
  const reusable = instance();
  async function request(component: Component, name: string, mode: number): Promise<void> {
    component.exports.configure(mode);
    const marker = { label: name };
    context.enterWith(marker);
    const pending = component.handleHttp(server);
    gc();
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
  await request(reusable, "large", 0);
  await request(reusable, "explicit-error", 3);
  await request(reusable, "reused", 0);
  reusable.dispose();
  for (const mode of [1, 2, 4]) {
    const component = instance({}, 100000000);
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
  console.log("PASS Perry HTTP guest");
}
main().catch(error => { console.error(error); process.exitCode = 1; });
