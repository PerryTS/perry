import tls from "node:tls";

const server = tls.createServer();
console.log(
  "state:",
  server instanceof tls.Server,
  server.listening,
  server.address(),
);
console.log(
  "events:",
  server.listenerCount("secureConnection"),
  server.eventNames().length,
);
for (
  const [label, value] of [["short", Buffer.alloc(47)], [
    "long",
    Buffer.alloc(49),
  ], ["string", "bad"]] as const
) {
  try {
    server.setTicketKeys(value as any);
    console.log(label + ": no throw");
  } catch (err: any) {
    console.log(
      label + ":",
      err instanceof TypeError || err instanceof RangeError,
    );
  }
}
for (
  const [label, value] of [
    ["null", null],
    ["undefined", undefined],
    ["number", 1],
    ["uint8", new Uint8Array(48)],
    ["dataview", new DataView(new ArrayBuffer(48))],
  ] as const
) {
  try {
    server.setTicketKeys(value as any);
    console.log(label + ": no throw");
  } catch (err: any) {
    console.log(label + ":", err instanceof TypeError, err.code ?? "none");
  }
}
console.log("ticket length:", server.getTicketKeys().length);
