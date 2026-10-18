import { AsyncLocalStorage } from "node:async_hooks";
import { createSocket } from "node:dgram";

const storage = new AsyncLocalStorage<{ label: string }>();
const server = createSocket("udp4");
const client = storage.run({ label: "creation" }, () => createSocket("udp4"));
const first = { label: "first" };
const second = { label: "second" };
const records: string[] = [];
const messages: string[] = [];

try {
  await new Promise<void>((resolve) => server.bind(0, "127.0.0.1", resolve));
  await storage.run({ label: "binding" }, () =>
    new Promise<void>((resolve) => client.bind(0, "127.0.0.1", resolve)),
  );
  const received = new Promise<void>((resolve) => {
    server.on("message", (message) => {
      messages.push(message.toString());
      if (messages.length === 3) resolve();
    });
  });
  const port = server.address().port;

  function send(label: string, expected: { label: string } | undefined) {
    let synchronous = true;
    const sent = new Promise<void>((resolve) => {
      client.send(label, port, "127.0.0.1", function (this: unknown, error, bytes) {
        const store = storage.getStore();
        records.push(
          `${label}:${store?.label ?? "undefined"}:${store === expected}:${this === undefined}:${arguments.length}:${error === null}:${bytes}:${!synchronous}`,
        );
        resolve();
      });
    });
    synchronous = false;
    return sent;
  }

  const sentFirst = storage.run(first, () => send("first", first));
  const sentSecond = storage.run(second, () => send("second", second));
  const sentOutside = send("outside", undefined);
  console.log("submission outside:", String(storage.getStore()));
  await Promise.all([sentFirst, sentSecond, sentOutside, received]);
  console.log("submission callbacks:", records.sort().join("|"));
  console.log("submission messages:", messages.sort().join(","));
  console.log("completion outside:", String(storage.getStore()));
} finally {
  await Promise.all([
    new Promise<void>((resolve) => client.close(resolve)),
    new Promise<void>((resolve) => server.close(resolve)),
  ]);
}
