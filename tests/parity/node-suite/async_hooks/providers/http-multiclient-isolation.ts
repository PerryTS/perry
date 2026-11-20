import { AsyncLocalStorage } from "node:async_hooks";
import { createServer, get } from "node:http";

type ClientStore = { id: string; body: string };
const storage = new AsyncLocalStorage<ClientStore>();
const responseStores = new Set<string>();
const dataStores = new Set<string>();
const endStores = new Set<string>();
let missingResponseStore = false;
let missingDataStore = false;
let missingEndStore = false;

function recordResponseStore() {
  const store = storage.getStore();
  if (store) responseStores.add(store.id);
  else missingResponseStore = true;
}
function onData(chunk: Buffer) {
  const store = storage.getStore();
  if (store) {
    dataStores.add(store.id);
    store.body += String(chunk);
  } else {
    missingDataStore = true;
  }
}
function recordEndStore() {
  const store = storage.getStore();
  if (store) endStores.add(store.id);
  else missingEndStore = true;
}

const server = createServer((request, response) => {
  const id = request.url?.slice(1) || "missing";
  response.write(id.repeat(64));
  setImmediate(() => response.end(id.repeat(64)));
});
const requests: ReturnType<typeof get>[] = [];
const ids = ["0", "1", "2", "3", "4"];
let bodies: string[] = [];
try {
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const address = server.address();
  if (!address || typeof address === "string")
    throw new Error("missing HTTP address");
  bodies = await Promise.all(
    ids.map((id) => {
      const store: ClientStore = { id, body: "" };
      return storage.run(
        store,
        () =>
          new Promise<string>((resolve, reject) => {
            const request = get(
              { host: "127.0.0.1", port: address.port, path: `/${id}` },
              (response) => {
                recordResponseStore();
                response.on("error", reject);
                response.on("data", onData);
                response.on("end", () => {
                  recordEndStore();
                  resolve(store.body);
                });
              },
            );
            requests.push(request);
            request.on("error", reject);
          }),
      );
    }),
  );
} finally {
  for (const request of requests) request.destroy();
  if (server.listening) {
    await new Promise<void>((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve())),
    );
  }
}

console.log(
  "http concurrent response stores:",
  [...responseStores].sort().join(","),
  missingResponseStore,
);
console.log(
  "http concurrent data stores:",
  [...dataStores].sort().join(","),
  missingDataStore,
);
console.log(
  "http concurrent end stores:",
  [...endStores].sort().join(","),
  missingEndStore,
);
console.log(
  "http concurrent bodies isolated:",
  bodies.map((body, index) => body === ids[index].repeat(128)).join(","),
);
console.log("http concurrent outside:", String(storage.getStore()));
