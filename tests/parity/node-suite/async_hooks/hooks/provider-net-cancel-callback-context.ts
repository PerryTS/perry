// Narrow public canceled-END context/receiver regression and WRITE context control.
// This does not prove native SHUTDOWNWRAP completion/lifetime or full WRITE
// error/order parity; those remain separate work, with wide diagnostics retained.
import { AsyncLocalStorage, createHook, executionAsyncId } from "node:async_hooks";
import { connect, createServer, type Socket } from "node:net";

const storage = new AsyncLocalStorage<string>();
const providers = new Map<number, string>();
const hook = createHook({
  init(id, type) {
    providers.set(id, type);
  },
}).enable();
const accepted = new Set<Socket>();
const server = createServer((socket) => {
  accepted.add(socket);
  socket.once("close", () => accepted.delete(socket));
  socket.end("ok");
});
await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
const address = server.address();
if (!address || typeof address === "string") throw new Error("missing net address");

let client: ReturnType<typeof connect> | undefined;
let endCalls = 0;
let writeCalls = 0;
let endFinishes = 0;
const endOrder: string[] = [];
const observations: string[] = [];
function observe(label: string, receiver: unknown, error?: Error & { code?: string }) {
  if (label === "write") {
    observations.push("write:" + (providers.get(executionAsyncId()) || "other") +
      ":" + String(storage.getStore()));
    return;
  }
  observations.push(
    label + ":" + String(error?.code || "none") + ":" +
      (providers.get(executionAsyncId()) || "other") + ":" +
      String(storage.getStore()) + ":" + String(receiver === undefined),
  );
}
try {
  // End while handling readable EOF, then destroy before its completion.
  await storage.run("end-owner", () => new Promise<void>((resolve, reject) => {
    client = connect(address.port, "127.0.0.1");
    client.once("error", reject);
    client.on("finish", () => endFinishes++);
    client.once("close", () => endOrder.push("close"));
    client.resume();
    client.once("end", () => {
      client!.end(function endCallback(this: unknown, error?: Error & { code?: string }) {
        endCalls++;
        endOrder.push("callback");
        observe("end", this, error);
        console.log("end receiver identity:", this === client, this === globalThis,
          Array.isArray(this), Array.isArray(this) ? this.length : -1,
          Array.isArray(this) ? this[0] === endCallback : false);
      });
      resolve();
    });
  }));
  const endClosed = new Promise<void>((resolve) => client!.once("close", resolve));
  storage.run("cancel-end", () => client!.destroy());
  await endClosed;
  console.log("canceled end:", endCalls, endFinishes, endOrder.join(">"));
  console.log("end outside store:", String(storage.getStore()));

  // A write canceled while connecting has its own public oracle; do not
  // infer its callback scope from the canceled end callback above.
  await new Promise<void>((resolve) => {
    storage.run("write-owner", () => {
      client = connect(address.port, "127.0.0.1");
      client.on("error", () => {});
      client.once("close", () => {
        resolve();
      });
      client.write("pending", function (this: unknown, error?: Error & { code?: string }) {
        writeCalls++;
        observe("write", this, error);
      });
      storage.run("cancel-write", () => client!.destroy());
    });
  });
  console.log("canceled write context count:", writeCalls);
  console.log("write outside store:", String(storage.getStore()));
  console.log("canceled callback observations:", observations.join(","));
} finally {
  client?.destroy();
  for (const socket of accepted) socket.destroy();
  if (server.listening) {
    await new Promise<void>((resolve, reject) =>
      server.close((error) => error ? reject(error) : resolve()),
    );
  }
  hook.disable();
  storage.disable();
}
