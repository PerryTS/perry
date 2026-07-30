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

const socket = dgram.createSocket({ type: "udp4", reuseAddr: true });
let isClosed = false;
socket.once("close", () => isClosed = true);
try {
  await new Promise<void>((resolve) => socket.bind(0, "127.0.0.1", resolve));
  console.log(
    "missing add:",
    codeOf(() => socket.addMembership(undefined as never)),
  );
  console.log(
    "missing drop:",
    codeOf(() => socket.dropMembership(undefined as never)),
  );
  console.log(
    "invalid add:",
    codeOf(() => socket.addMembership("256.256.256.256")),
  );
  console.log(
    "invalid drop:",
    codeOf(() => socket.dropMembership("256.256.256.256")),
  );
  console.log(
    "invalid source:",
    codeOf(() => socket.addSourceSpecificMembership(0 as never, "224.0.0.114")),
  );
  console.log(
    "invalid group:",
    codeOf(() =>
      socket.dropSourceSpecificMembership("224.0.0.114", 0 as never)
    ),
  );

  await new Promise<void>((resolve) => socket.close(resolve));
  console.log("closed add:", codeOf(() => socket.addMembership("224.0.0.114")));
  console.log(
    "closed drop:",
    codeOf(() => socket.dropMembership("224.0.0.114")),
  );
} finally {
  if (!isClosed) {
    await new Promise<void>((resolve) => socket.close(resolve));
  }
}
