// #11227: Node calls every EventEmitter listener with `this` bound to the
// emitter. Perry's net pump bound it only for 'connect'/'drain'; 'close',
// 'data', 'end', 'error' and every server event ran `function` listeners
// with `this === undefined` (undici's `onHttpSocketClose` reads
// `this[kParser]`, so `Client.close()` crashed). A net.Server also had no
// prependListener/prependOnceListener, and sockets never emitted 'ready'.
import * as net from "node:net";

const seen = new Set<string>();
function rec(label: string, target: any) {
  return function (this: any) {
    seen.add(`${label}: ${this === target ? "self" : typeof this}`);
  };
}
function watch(prefix: string, target: any, events: string[]) {
  for (const ev of events) {
    target.on(ev, rec(`${prefix} on ${ev}`, target));
    target.once(ev, rec(`${prefix} once ${ev}`, target));
    target.prependListener(ev, rec(`${prefix} prepend ${ev}`, target));
    target.prependOnceListener(ev, rec(`${prefix} prependOnce ${ev}`, target));
    target.addListener(ev, rec(`${prefix} addListener ${ev}`, target));
  }
}
const wait = (t: any, ev: string) => new Promise<void>((r) => t.once(ev, () => r()));

async function main() {
  // Typed receivers take the codegen's static rows, `any` the dynamic path.
  const typed: net.Server = net.createServer();
  typed.prependListener("listening", function (this: any) {
    seen.add(`typed server prepend listening: ${this === typed ? "self" : typeof this}`);
  });
  typed.prependOnceListener("close", function (this: any) {
    seen.add(`typed server prependOnce close: ${this === typed ? "self" : typeof this}`);
  });
  const typedListening = wait(typed, "listening");
  typed.listen(0, "127.0.0.1");
  await typedListening;
  const typedClosed = wait(typed, "close");
  typed.close();
  await typedClosed;

  const server: any = net.createServer();
  watch("server", server, ["listening", "connection", "close"]);
  server.on("connection", (s: any) => {
    watch("accepted", s, ["data", "end", "close"]);
    s.on("data", (d: any) => {
      if (d.length === 2) s.end("bye");
    });
  });
  const listening = wait(server, "listening");
  server.listen(0, "127.0.0.1");
  await listening;
  const port = server.address().port;

  const c: any = new net.Socket();
  watch("client", c, ["connect", "ready", "data", "end", "close", "custom"]);
  const connected = wait(c, "connect");
  c.connect(port, "127.0.0.1");
  await connected;
  c.emit("custom", 1, 2, 3);
  const closed = wait(c, "close");
  c.write("hi");
  await closed;

  const d: any = net.connect(port, "127.0.0.1");
  watch("destroyed", d, ["close"]);
  await wait(d, "connect");
  const dclosed = wait(d, "close");
  d.destroy();
  await dclosed;

  const t: any = net.connect(port, "127.0.0.1");
  watch("writer", t, ["drain"]);
  await wait(t, "connect");
  const drained = wait(t, "drain");
  let full = false;
  const chunk = Buffer.alloc(1 << 20, 97);
  for (let i = 0; i < 64 && !full; i++) full = !t.write(chunk);
  await drained;
  const tclosed = wait(t, "close");
  t.destroy();
  await tclosed;

  const e: any = new net.Socket();
  watch("refused", e, ["error", "close"]);
  const eclosed = wait(e, "close");
  e.connect(1, "127.0.0.1");
  await eclosed;

  const s2: any = net.createServer();
  watch("inuse", s2, ["error"]);
  const s2err = wait(s2, "error");
  s2.listen(port, "127.0.0.1");
  await s2err;

  const sclosed = wait(server, "close");
  server.close();
  await sclosed;
  await new Promise((r) => setTimeout(r, 20));
  for (const line of [...seen].sort()) console.log(line);
  console.log("lines:", seen.size);
}
main();
