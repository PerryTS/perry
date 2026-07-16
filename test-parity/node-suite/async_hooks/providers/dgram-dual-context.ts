import { AsyncLocalStorage } from "node:async_hooks";
import { createSocket } from "node:dgram";

const storage = new AsyncLocalStorage<string>();
let serverMessageStore: string | undefined;
let clientSendStore: string | undefined;
let clientMessageStore: string | undefined;
const server = storage.run("server-context", () => createSocket("udp4"));
const client = storage.run("client-context", () => createSocket("udp4"));

function closeSocket(socket: typeof server) {
  return new Promise<void>((resolve) => {
    try {
      socket.close(resolve);
    } catch {
      resolve();
    }
  });
}

try {
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    client.once("error", reject);
    server.on("message", (message, remote) => {
      serverMessageStore = storage.getStore();
      server.send(message, remote.port, remote.address);
    });
    client.once("message", () => {
      clientMessageStore = storage.getStore();
      resolve();
    });
    storage.run("server-context", () => {
      server.bind(0, "127.0.0.1", () => {
        const address = server.address();
        storage.run("client-context", () => {
          client.send("dual-context", address.port, "127.0.0.1", (error) => {
            clientSendStore = storage.getStore();
            if (error) reject(error);
          });
        });
      });
    });
  });
} finally {
  await Promise.all([closeSocket(client), closeSocket(server)]);
}

console.log(
  "dgram dual stores:",
  serverMessageStore,
  clientSendStore,
  clientMessageStore,
);
console.log("dgram dual outside:", String(storage.getStore()));
