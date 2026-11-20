// Upstream: Node v26.5.0 test/parallel/test-dgram-send-multi-buffer-copy.js.
// Coverage added: scatter array ownership after send().
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
try {
  await new Promise<void>((resolve) => socket.bind(0, "127.0.0.1", resolve));
  const parts = [Buffer.from("copy-"), Buffer.from("safe")];
  let settleReceived: (value: string) => void = () => {};
  const received = new Promise<string>((resolve) => settleReceived = resolve);
  const onMessage = (message: Buffer) => settleReceived(message.toString());
  socket.once("message", onMessage);
  let settleSent: (value: number) => void = () => {};
  const sent = new Promise<number>((resolve) => settleSent = resolve);
  const accepted = codeOf(() => {
    socket.send(parts, socket.address().port, "127.0.0.1", (error, bytes) => {
      settleSent(error ? -1 : bytes);
    });
  });
  console.log("scatter copy accepted:", accepted);
  parts.splice(0, parts.length);
  if (accepted === "none") {
    console.log("scatter copy:", await received, await sent, parts.length);
  } else {
    socket.removeListener("message", onMessage);
  }
} finally {
  await new Promise<void>((resolve) => socket.close(resolve));
}
