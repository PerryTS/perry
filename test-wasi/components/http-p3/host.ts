import { readFileSync } from "node:fs";
import { AsyncLocalStorage } from "node:async_hooks";
import { createComponentHttpServer, closeComponentHttpServer } from "perry/component";
import { load } from "./generated/bindings";
import type { Component } from "./generated/bindings";
declare function gc(): void;
function check(ok: boolean, message: string): void { if (!ok) throw new Error(message); }
const root = process.argv[2];
const tls = process.argv[3] === "tls";
const context = new AsyncLocalStorage<{ label: string }>();
let marker = { label: "initial" };
let calls = 0;
function listener() {
  return createComponentHttpServer({ address: "127.0.0.1:0", maxConnections: 1,
    requestHeadTimeoutMs: 5000, responseHeadTimeoutMs: 5000, betweenBytesTimeoutMs: 5000,
    lingerTimeoutMs: 10,
    ...(tls ? { tls: { certificatePem: readFileSync(process.argv[4], "utf8"), privateKeyPem: readFileSync(process.argv[5], "utf8") } } : {}) });
}
const reason = { label: "original async HTTP rejection" };
function instance(name: string, mode: string, server: ReturnType<typeof listener>): Component {
  let component: Component;
  component = load(readFileSync(root + "/" + name + ".wasm"), {
    before: async (): Promise<void> => {
      check(context.getStore() === marker, "async import starts in request context");
      calls++;
      gc();
      await new Promise<void>(resolve => setTimeout(() => { gc(); resolve(); }, 5));
      check(context.getStore() === marker, "async import resumes in request context");
      if (mode === "reject") throw reason;
      if (mode === "dispose") component.dispose();
      if (mode === "close") {
        closeComponentHttpServer(server);
        await new Promise<void>(() => {});
      }
    },
  }, { wasi: {}, fuel: 1000000 });
  return component;
}
async function request(component: Component, server: ReturnType<typeof listener>, name: string, mode: string): Promise<void> {
  marker = { label: name };
  context.enterWith(marker);
  calls = 0;
  const pending = component.handleHttp(server);
  gc();
  await new Promise<void>(resolve => setTimeout(() => { gc(); resolve(); }, 5));
  console.log("READY " + name + " " + server.address);
  let error: unknown;
  try { await pending; } catch (caught) { error = caught; }
  check((error !== undefined) === (mode !== "ok"), "request result " + name);
  if (mode === "reject") check(error === reason, "original import rejection identity");
  check(context.getStore() === marker, "request completion context " + name);
  check(calls === ((mode === "reject" || mode === "dispose" || mode === "close") ? 1 : 2), "real async imports " + name);
  gc();
  console.log("RESULT " + name);
}
async function main(): Promise<void> {
  const server = listener();
  const reusable = instance("echo", "ok", server);
  await request(reusable, server, "empty", "ok");
  await request(reusable, server, "large", "ok");
  await request(reusable, server, "reused", "ok");
  reusable.dispose();
  const denied = instance("denied", "ok", server);
  await request(denied, server, "denied", "denied");
  await request(denied, server, "denied-reused", "denied");
  denied.dispose();
  for (const kind of ["trap", "late-trap", "reject", "dispose"]) {
    const component = instance(kind === "trap" || kind === "late-trap" ? kind : "echo", kind, server);
    await request(component, server, kind, kind);
    component.dispose();
  }
  closeComponentHttpServer(server);
  const closing = listener();
  const component = instance("echo", "close", closing);
  await request(component, closing, "close", "close");
  component.dispose(); closeComponentHttpServer(closing);
  console.log("PASS P3 HTTP: async imports, streams, trailers, reuse, errors, cancellation, ALS");
}
main().catch(error => { console.error(error); process.exitCode = 1; });
