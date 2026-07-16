import * as dgram from "node:dgram";

const receiver = dgram.createSocket("udp4");
await new Promise<void>((resolve) => receiver.bind(0, "127.0.0.1", () => resolve()));
const sender = dgram.createSocket("udp4");

async function send(address: string | null | undefined) {
  return await new Promise<string>((resolve) => {
    sender.send("default", receiver.address().port, address as never, (error, bytes) => {
      resolve(`${error?.code ?? "null"}:${bytes ?? "none"}`);
    });
  });
}

console.log("empty:", await send(""));
console.log("null:", await send(null));
console.log("undefined:", await send(undefined));

await Promise.all([
  new Promise<void>((resolve) => sender.close(() => resolve())),
  new Promise<void>((resolve) => receiver.close(() => resolve())),
]);
