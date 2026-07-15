import * as dgram from "node:dgram";

const receiver = dgram.createSocket("udp4");
await new Promise<void>((resolve) => receiver.bind(0, "127.0.0.1", () => resolve()));
const sender = dgram.createSocket("udp4");
await new Promise<void>((resolve) => {
  sender.connect(receiver.address().port, "127.0.0.1", () => resolve());
});

async function roundTrip(label: string, message: string | Uint8Array) {
  const received = new Promise<string>((resolve) => {
    receiver.once("message", (value) => resolve(value.toString()));
  });
  const callback = new Promise<string>((resolve) => {
    sender.send(message, (error, bytes) => resolve(`${error === null}:${bytes}`));
  });
  console.log(label, await received, await callback);
}

await roundTrip("string", "connected string");
await roundTrip("typed", new Uint8Array([116, 121, 112, 101, 100]));

await Promise.all([
  new Promise<void>((resolve) => sender.close(() => resolve())),
  new Promise<void>((resolve) => receiver.close(() => resolve())),
]);
