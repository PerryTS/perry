import * as dgram from "node:dgram";

const acceptedSockets: dgram.Socket[] = [];
function codeOf(fn: () => unknown): string {
  try {
    const socket = fn() as dgram.Socket | undefined;
    if (socket) acceptedSockets.push(socket);
    return "none";
  } catch (error: unknown) {
    return (error as { code?: string; name?: string }).code ??
      (error as { name?: string }).name ?? "Error";
  }
}

console.log(
  "invalid lookup:",
  [null, true, 0, "lookup", {}]
    .map((lookup) => codeOf(() => dgram.createSocket({ type: "udp4", lookup: lookup as never })))
    .join(","),
);
console.log(
  "invalid recv size:",
  codeOf(() => dgram.createSocket({ type: "udp4", recvBufferSize: "bad" as never })),
);
console.log(
  "invalid send size:",
  codeOf(() => dgram.createSocket({ type: "udp4", sendBufferSize: "bad" as never })),
);

await Promise.all(
  acceptedSockets.map((socket) => new Promise<void>((resolve) => socket.close(() => resolve()))),
);
