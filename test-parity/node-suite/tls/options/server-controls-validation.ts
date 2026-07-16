import tls from "node:tls";

const server = tls.createServer();
console.log("state:", server instanceof tls.Server, server.listening, server.address());
console.log("events:", server.listenerCount("secureConnection"), server.eventNames().length);
for (const [label, value] of [["short", Buffer.alloc(47)], ["long", Buffer.alloc(49)], ["string", "bad"]] as const) {
  try {
    server.setTicketKeys(value as any);
    console.log(label + ": no throw");
  } catch (err: any) {
    console.log(label + ":", err instanceof TypeError || err instanceof RangeError, err.code);
  }
}
console.log("ticket length:", server.getTicketKeys().length);
