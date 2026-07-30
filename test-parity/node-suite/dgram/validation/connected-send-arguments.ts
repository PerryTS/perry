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

const peer = dgram.createSocket("udp4");
const socket = dgram.createSocket("udp4");
try {
  await new Promise<void>((resolve) =>
    peer.bind(0, "127.0.0.1", () => resolve())
  );
  await new Promise<void>((resolve) => {
    socket.connect(peer.address().port, "127.0.0.1", () => resolve());
  });

  const destinationMessage = new Promise<string>((resolve) => {
    peer.once("message", (message) => resolve(message.toString()));
  });
  console.log(
    "destination while connected:",
    codeOf(() => socket.send("x", peer.address().port, "127.0.0.1")),
  );
  console.log("three-argument payload:", await destinationMessage);

  const rangeMessage = new Promise<string>((resolve) => {
    peer.once("message", (message) => resolve(message.toString()));
  });
  const rangeCallback = new Promise<string>((resolve) => {
    socket.send(Buffer.from("slice"), 1, 3, (error, bytes) => {
      resolve(`${error === null}:${bytes}`);
    });
  });
  console.log("connected range:", await rangeMessage, await rangeCallback);

  console.log(
    "range destination while connected:",
    codeOf(() =>
      socket.send(Buffer.from("x"), 0, 1, peer.address().port, "127.0.0.1")
    ),
  );
} finally {
  await Promise.all([
    new Promise<void>((resolve) => socket.close(() => resolve())),
    new Promise<void>((resolve) => peer.close(() => resolve())),
  ]);
}
