import { AsyncLocalStorage } from "node:async_hooks";
import { connect, createServer, type Socket } from "node:net";

type ClientStore = { id: string };
const storage = new AsyncLocalStorage<ClientStore>();
const dataStores = new Set<string>();
const endStores = new Set<string>();
let missingDataStore = false;
let missingEndStore = false;

function recordDataStore() {
  const store = storage.getStore();
  if (store) dataStores.add(store.id);
  else missingDataStore = true;
}
function recordEndStore() {
  const store = storage.getStore();
  if (store) endStores.add(store.id);
  else missingEndStore = true;
}

const serverSockets = new Set<Socket>();
const server = createServer((socket) => {
  serverSockets.add(socket);
  socket.on("close", () => serverSockets.delete(socket));
  socket.once("data", (chunk) => {
    const id = String(chunk);
    socket.write(id);
    setImmediate(() => socket.end(id));
  });
});
const clients: Socket[] = [];
const ids = ["a", "b", "c", "d"];
let bodies: string[] = [];
try {
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const address = server.address();
  if (!address || typeof address === "string")
    throw new Error("missing net address");
  bodies = await Promise.all(
    ids.map((id) =>
      storage.run(
        { id },
        () =>
          new Promise<string>((resolve, reject) => {
            let body = "";
            const client = connect(address.port, "127.0.0.1", () => {
              client.end(id);
            });
            clients.push(client);
            client.on("data", (chunk) => {
              recordDataStore();
              body += String(chunk);
            });
            client.on("end", () => {
              recordEndStore();
            });
            client.on("close", () => resolve(body));
            client.on("error", reject);
          }),
      ),
    ),
  );
} finally {
  for (const client of clients) client.destroy();
  for (const socket of serverSockets) socket.destroy();
  if (server.listening) {
    await new Promise<void>((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve())),
    );
  }
}

console.log(
  "net concurrent data stores:",
  [...dataStores].sort().join(","),
  missingDataStore,
);
console.log(
  "net concurrent end stores:",
  [...endStores].sort().join(","),
  missingEndStore,
);
console.log(
  "net concurrent bodies isolated:",
  bodies.map((body, index) => body === ids[index].repeat(2)).join(","),
);
console.log("net concurrent outside:", String(storage.getStore()));
