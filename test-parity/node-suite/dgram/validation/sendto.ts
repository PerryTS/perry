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
console.log("missing args:", codeOf(() => socket.sendto()));
console.log(
  "bad offset:",
  codeOf(() => socket.sendto("buffer", "offset" as never, 1, 12345, "127.0.0.1")),
);
console.log(
  "bad length:",
  codeOf(() => socket.sendto("buffer", 1, "length" as never, 12345, "127.0.0.1")),
);
console.log(
  "bad port:",
  codeOf(() => socket.sendto("buffer", 1, 1, false as never, "127.0.0.1")),
);
console.log(
  "bad address:",
  codeOf(() => socket.sendto("buffer", 1, 1, 12345, false as never)),
);
await new Promise<void>((resolve) => socket.close(() => resolve()));
