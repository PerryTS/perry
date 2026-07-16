import {
  createHook,
  executionAsyncId,
  executionAsyncResource,
} from "node:async_hooks";
import { get, createServer } from "node:http";

const resources = new Map<number, object>();
const providerTypes = new Set(["HTTPCLIENTREQUEST", "HTTPINCOMINGMESSAGE"]);
type ProviderEntry = {
  id: number;
  type: string;
  before: number;
  after: number;
  destroy: number;
};
const providerEntries: ProviderEntry[] = [];
const providerById = new Map<number, ProviderEntry>();
const hook = createHook({
  init(asyncId, type, _triggerAsyncId, resource) {
    resources.set(asyncId, resource);
    if (providerTypes.has(type)) {
      const entry = { id: asyncId, type, before: 0, after: 0, destroy: 0 };
      providerEntries.push(entry);
      providerById.set(asyncId, entry);
    }
  },
  before(asyncId) {
    const entry = providerById.get(asyncId);
    if (entry) entry.before++;
  },
  after(asyncId) {
    const entry = providerById.get(asyncId);
    if (entry) entry.after++;
  },
  destroy(asyncId) {
    const entry = providerById.get(asyncId);
    if (entry) entry.destroy++;
  },
}).enable();
let incomingProviderMapped = false;
const server = createServer((_request, response) => {
  incomingProviderMapped = providerEntries.some(
    (entry) =>
      entry.type === "HTTPINCOMINGMESSAGE" &&
      entry.id === executionAsyncId() &&
      resources.get(entry.id) === executionAsyncResource(),
  );
  response.end("ok");
});
let request: ReturnType<typeof get> | undefined;
let completed = false;
let clientProviderMapped = false;

try {
  await new Promise<void>((resolve, reject) => {
    const timeout = setTimeout(resolve, 1_000);
    const finish = () => {
      completed = true;
      clearTimeout(timeout);
      resolve();
    };
    const fail = (error: Error) => {
      clearTimeout(timeout);
      reject(error);
    };
    server.once("error", fail);
    server.listen(0, "127.0.0.1", () => {
      console.log(
        "http listen resource mapped:",
        executionAsyncResource() === resources.get(executionAsyncId()),
      );
      const address = server.address();
      if (!address || typeof address === "string") {
        fail(new Error("missing address"));
        return;
      }
      request = get({ host: "127.0.0.1", port: address.port }, (response) => {
        clientProviderMapped = providerEntries.some(
          (entry) =>
            entry.type === "HTTPCLIENTREQUEST" &&
            entry.id === executionAsyncId() &&
            resources.get(entry.id) === executionAsyncResource(),
        );
        console.log(
          "http response resource mapped:",
          executionAsyncResource() === resources.get(executionAsyncId()),
        );
        response.once("error", fail);
        response.resume();
        response.once("end", finish);
      });
      request.once("error", fail);
    });
  });
} finally {
  request?.destroy();
  if (server.listening) {
    await new Promise<void>((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve())),
    );
  }
  await new Promise<void>((resolve) => setImmediate(resolve));
  await new Promise<void>((resolve) => setImmediate(resolve));
  hook.disable();
}
console.log("http execution resource completed:", completed);
console.log(
  "http provider mappings:",
  clientProviderMapped,
  incomingProviderMapped,
);
for (const type of providerTypes) {
  const selected = providerEntries.filter((entry) => entry.type === type);
  console.log(
    `${type} provider lifecycle:`,
    selected.length,
    selected.length === 1 &&
      selected.every(
        (entry) =>
          entry.before > 0 &&
          entry.before === entry.after &&
          entry.destroy === 1,
      ),
  );
}
