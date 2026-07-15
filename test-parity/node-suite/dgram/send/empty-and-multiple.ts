import * as dgram from "node:dgram";

const receiver = dgram.createSocket("udp4");
await new Promise<void>((resolve) => receiver.bind(0, "127.0.0.1", () => resolve()));
const port = receiver.address().port;
const sender = dgram.createSocket("udp4");

async function roundTrip(
  label: string,
  send: (callback: (error: Error | null, bytes: number) => void) => void,
) {
  const message = new Promise<Buffer>((resolve) => receiver.once("message", resolve));
  const sent = new Promise<string>((resolve) => {
    send((error, bytes) => resolve(`${error === null}:${bytes}`));
  });
  const received = await message;
  console.log(label, received.length, await sent);
}

await roundTrip("empty buffer", (callback) => {
  sender.send(Buffer.alloc(0), port, "127.0.0.1", callback);
});
const messages: string[] = [];
const multipleReceived = new Promise<string>((resolve) => {
  receiver.on("message", (message) => {
    messages.push(message.toString());
    if (messages.length === 2) resolve(messages.sort().join(","));
  });
});
await Promise.all([
  new Promise<void>((resolve) => sender.send("first", port, "127.0.0.1", () => resolve())),
  new Promise<void>((resolve) => sender.send("second", port, "127.0.0.1", () => resolve())),
]);
console.log("multiple sends:", await multipleReceived);

await Promise.all([
  new Promise<void>((resolve) => sender.close(() => resolve())),
  new Promise<void>((resolve) => receiver.close(() => resolve())),
]);
