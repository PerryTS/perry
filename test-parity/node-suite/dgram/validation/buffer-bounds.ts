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

const socket = dgram.createSocket("udp4");
await new Promise<void>((resolve) => socket.bind(0, "127.0.0.1", resolve));
const receiver = dgram.createSocket("udp4");
await new Promise<void>((resolve) => receiver.bind(0, "127.0.0.1", resolve));
const message = Buffer.from("hello");
const callback = () => {};
const port = receiver.address().port;

console.log(
  "offset:",
  codeOf(() => socket.send(message, 6, 0, port, "127.0.0.1", callback)),
);
console.log(
  "length:",
  codeOf(() => socket.send(message, 0, 6, port, "127.0.0.1", callback)),
);
console.log(
  "combined:",
  codeOf(() => socket.send(message, 3, 4, port, "127.0.0.1", callback)),
);

await Promise.all([
  new Promise<void>((resolve) => socket.close(resolve)),
  new Promise<void>((resolve) => receiver.close(resolve)),
]);
