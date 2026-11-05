import * as dgram from "node:dgram";

const socket = dgram.createSocket("udp4");
let closeEvents = 0;
const onClose = () => closeEvents++;
socket.on("close", onClose);
const closed = new Promise<void>((resolve) => {
  socket.once("close", () => queueMicrotask(resolve));
});

try {
  const result = (socket.close as (callback?: unknown) => dgram.Socket)(
    "not a callback",
  );
  await closed;
  console.log("close result self:", result === socket);
  console.log("close events:", closeEvents);
} finally {
  if (closeEvents === 0) {
    await new Promise<void>((resolve) => socket.close(resolve));
  }
  socket.removeListener("close", onClose);
}
