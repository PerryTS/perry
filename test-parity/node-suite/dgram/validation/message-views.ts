import * as dgram from "node:dgram";

function codeOf(fn: () => unknown): string {
  try {
    fn();
    return "none";
  } catch (error: unknown) {
    return (error as { code?: string; name?: string }).code ??
      (error as { name?: string }).name ?? "Error";
  }
}

const receiver = dgram.createSocket("udp4");
await new Promise<void>((resolve) => receiver.bind(0, "127.0.0.1", () => resolve()));
const sender = dgram.createSocket("udp4");
const data = new ArrayBuffer(4);
new Uint8Array(data).set([118, 105, 101, 119]);

const viewReceived = new Promise<string>((resolve) => {
  receiver.once("message", (message) => resolve(message.toString()));
});
const viewCallback = new Promise<string>((resolve) => {
  sender.send(new DataView(data), receiver.address().port, "127.0.0.1", (error, bytes) => {
    resolve(`${error === null}:${bytes}`);
  });
});
console.log("data view:", await viewReceived, await viewCallback);

let finishScatter: (value: string) => void = () => {};
const scatterCallback = new Promise<string>((resolve) => {
  finishScatter = resolve;
});
const scatterReceived = new Promise<string>((resolve) => {
  receiver.once("message", (message) => resolve(message.toString()));
});
const scatterResult = codeOf(() => {
  sender.send([Buffer.from("a"), "b"], receiver.address().port, "127.0.0.1", (error, bytes) => {
    finishScatter(`${error === null}:${bytes}`);
  });
});
console.log("scatter accepted:", scatterResult);
if (scatterResult === "none") {
  console.log("scatter delivery:", await scatterReceived, await scatterCallback);
}

await Promise.all([
  new Promise<void>((resolve) => sender.close(() => resolve())),
  new Promise<void>((resolve) => receiver.close(() => resolve())),
]);
