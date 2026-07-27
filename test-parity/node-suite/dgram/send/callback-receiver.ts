// Upstream: Node v26.5.0 lib/dgram.js doSend()/afterSend() callback paths.
// Coverage added: send callback receiver and arity.
import * as dgram from "node:dgram";

const receiver = dgram.createSocket("udp4");
const sender = dgram.createSocket("udp4");
try {
  await new Promise<void>((resolve) => receiver.bind(0, "127.0.0.1", resolve));
  const received = new Promise<void>((resolve) =>
    receiver.once("message", () => resolve())
  );
  const callback = new Promise<string>((resolve) => {
    sender.send(
      "x",
      receiver.address().port,
      "127.0.0.1",
      function (this: unknown, error, bytes) {
        resolve(
          `${this === undefined}:${
            error === null
          }:${bytes}:${arguments.length}`,
        );
      },
    );
  });
  console.log("send callback:", await callback);
  await received;
} finally {
  await Promise.all([
    new Promise<void>((resolve) => sender.close(resolve)),
    new Promise<void>((resolve) => receiver.close(resolve)),
  ]);
}
