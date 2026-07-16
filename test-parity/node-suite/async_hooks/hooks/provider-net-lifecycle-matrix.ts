import { createHook } from "node:async_hooks";
import { connect, createServer, type Socket } from "node:net";

const tracked = new Set([
  "TCPSERVERWRAP",
  "TCPWRAP",
  "TCPCONNECTWRAP",
  "SHUTDOWNWRAP",
]);
type Entry = {
  asyncId: number;
  type: string;
  triggerAsyncId: number;
  before: number;
  after: number;
  destroy: number;
};
const entries: Entry[] = [];
const byId = new Map<number, Entry>();
const hook = createHook({
  init(asyncId, type, triggerAsyncId) {
    if (!tracked.has(type)) return;
    const entry = {
      asyncId,
      type,
      triggerAsyncId,
      before: 0,
      after: 0,
      destroy: 0,
    };
    entries.push(entry);
    byId.set(asyncId, entry);
  },
  before(asyncId) {
    const entry = byId.get(asyncId);
    if (entry) entry.before++;
  },
  after(asyncId) {
    const entry = byId.get(asyncId);
    if (entry) entry.after++;
  },
  destroy(asyncId) {
    const entry = byId.get(asyncId);
    if (entry) entry.destroy++;
  },
}).enable();

const serverSockets = new Set<Socket>();
const server = createServer((socket) => {
  serverSockets.add(socket);
  socket.once("close", () => serverSockets.delete(socket));
  socket.end("ok");
});
let client: ReturnType<typeof connect> | undefined;
try {
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      if (!address || typeof address === "string") {
        reject(new Error("missing net address"));
        return;
      }
      client = connect(address.port, "127.0.0.1");
      client.once("error", reject);
      client.resume();
      client.once("end", resolve);
    });
  });
} finally {
  client?.destroy();
  for (const socket of serverSockets) socket.destroy();
  if (server.listening) {
    await new Promise<void>((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve())),
    );
  }
  await new Promise<void>((resolve) => setImmediate(resolve));
  await new Promise<void>((resolve) => setImmediate(resolve));
  hook.disable();
}

const serverEntry = entries.find((entry) => entry.type === "TCPSERVERWRAP");
const tcpEntries = entries.filter((entry) => entry.type === "TCPWRAP");
const connectEntries = entries.filter(
  (entry) => entry.type === "TCPCONNECTWRAP",
);
const shutdownEntries = entries.filter(
  (entry) => entry.type === "SHUTDOWNWRAP",
);
const tcpIds = new Set(tcpEntries.map((entry) => entry.asyncId));
console.log(
  "net hook provider counts:",
  serverEntry ? 1 : 0,
  tcpEntries.length,
  connectEntries.length,
  shutdownEntries.length,
);
console.log(
  "net hook provider ancestry:",
  !!serverEntry &&
    tcpEntries.length === 2 &&
    tcpEntries.some((entry) => entry.triggerAsyncId === serverEntry.asyncId),
  connectEntries.length === 1 &&
    connectEntries.every((entry) => tcpIds.has(entry.triggerAsyncId)),
  shutdownEntries.length === 2 &&
    shutdownEntries.every((entry) => tcpIds.has(entry.triggerAsyncId)),
);
console.log(
  "net hook provider lifecycles:",
  entries.length > 0 &&
    entries.every(
      (entry) =>
        entry.before > 0 && entry.before === entry.after && entry.destroy === 1,
    ),
);
