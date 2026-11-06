import { AsyncLocalStorage } from "node:async_hooks";
import { connect, createServer, type Socket } from "node:net";

const storage = new AsyncLocalStorage<string>();
let serverConnectionStore: string | undefined;
let clientConnectStore: string | undefined;
const serverSockets = new Set<Socket>();
const server = storage.run("server-context", () =>
  createServer((socket) => {
    serverSockets.add(socket);
    socket.once("close", () => serverSockets.delete(socket));
    serverConnectionStore = storage.getStore();
    socket.end("ok");
  }),
);
let client: ReturnType<typeof connect> | undefined;

try {
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    storage.run("server-context", () => {
      server.listen(0, "127.0.0.1", () => {
        const address = server.address();
        if (!address || typeof address === "string") {
          reject(new Error("missing net address"));
          return;
        }
        storage.run("client-context", () => {
          client = connect(address.port, "127.0.0.1", () => {
            clientConnectStore = storage.getStore();
          });
          client.once("error", reject);
          client.once("data", () => resolve());
        });
      });
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
}

console.log("net dual stores:", serverConnectionStore, clientConnectStore);
console.log("net dual outside:", String(storage.getStore()));
