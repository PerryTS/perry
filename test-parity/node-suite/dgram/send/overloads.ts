import * as dgram from "node:dgram";

const receiver = dgram.createSocket("udp4");
await new Promise<void>((resolve) => receiver.bind(0, "127.0.0.1", () => resolve()));
const port = receiver.address().port;
const sender = dgram.createSocket("udp4");

async function sendAndReceive(
  expected: string,
  send: (callback: (error: Error | null, bytes: number) => void) => void,
) {
  const received = new Promise<string>((resolve) => {
    receiver.once("message", (message) => resolve(message.toString()));
  });
  const callback = new Promise<string>((resolve) => {
    send((error, bytes) => resolve(`${error === null}:${bytes}`));
  });
  console.log(expected, await received, await callback);
}

await sendAndReceive("string", (callback) => {
  sender.send("string", port, "127.0.0.1", callback);
});

await sendAndReceive("typed", (callback) => {
  sender.send(new Uint8Array([116, 121, 112, 101, 100]), port, "127.0.0.1", callback);
});

await Promise.all([
  new Promise<void>((resolve) => sender.close(() => resolve())),
  new Promise<void>((resolve) => receiver.close(() => resolve())),
]);
