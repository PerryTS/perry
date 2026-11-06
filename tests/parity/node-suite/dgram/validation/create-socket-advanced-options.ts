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

try {
  console.log(
    "invalid lookup:",
    [null, true, 0, "lookup", {}]
      .map((lookup) =>
        codeOf(() =>
          dgram.createSocket({ type: "udp4", lookup: lookup as never })
        )
      )
      .join(","),
  );
  console.log(
    "invalid recv size:",
    codeOf(() =>
      dgram.createSocket({ type: "udp4", recvBufferSize: "bad" as never })
    ),
  );
  console.log(
    "invalid send size:",
    codeOf(() =>
      dgram.createSocket({ type: "udp4", sendBufferSize: "bad" as never })
    ),
  );
  console.log(
    "invalid receive block list:",
    codeOf(() =>
      dgram.createSocket({ type: "udp4", receiveBlockList: {} as never })
    ),
  );
  console.log(
    "invalid send block list:",
    codeOf(() =>
      dgram.createSocket({ type: "udp4", sendBlockList: {} as never })
    ),
  );
} finally {
  for (const socket of acceptedSockets) {
    await new Promise<void>((resolve) => socket.bind(0, "127.0.0.1", resolve));
    const closed = new Promise<void>((resolve) =>
      socket.once("close", resolve)
    );
    socket.close();
    await closed;
  }
}
