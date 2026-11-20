// #3693: node:dgram default import + Socket EventEmitter/error shape.
//
// Deterministic shape/validation parity (no real ports printed): the default
// import resolves and equals the namespace default, createSocket validates the
// socket type, the stub Socket carries the EventEmitter surface (including
// eventNames), pre-bind / pre-connect calls raise Node error codes, and
// ref()/unref() return the socket.
import dgram from "node:dgram";
import * as dgramNs from "node:dgram";

function codeOf(fn: () => unknown): string {
  try {
    const value = fn();
    if (value === undefined) return "undefined";
    if (value === null) return "null";
    return typeof value;
  } catch (err: unknown) {
    const e = err as { code?: string; name?: string };
    return e.code ?? e.name ?? "Error";
  }
}

console.log("default type:", typeof dgram);
console.log("namespace createSocket:", typeof dgramNs.createSocket);
console.log(
  "default identity:",
  dgram === (dgramNs as { default?: unknown }).default,
);
console.log("bad type:", codeOf(() => dgram.createSocket("udp5" as never)));

const socket = dgram.createSocket("udp4");
const onMessage = () => {};
const onError = () => {};
try {
  for (
    const name of [
      "send",
      "bind",
      "close",
      "address",
      "connect",
      "disconnect",
      "addMembership",
      "dropMembership",
      "setBroadcast",
      "setTTL",
      "getSendQueueSize",
      "getSendQueueCount",
      "ref",
      "unref",
      "on",
      "once",
      "emit",
      "eventNames",
    ]
  ) {
    console.log(`${name}:`, typeof (socket as Record<string, unknown>)[name]);
  }

  console.log("address before bind:", codeOf(() => socket.address()));
  console.log("disconnect before connect:", codeOf(() => socket.disconnect()));
  console.log("ref returns self:", socket.ref() === socket);
  console.log("unref returns self:", socket.unref() === socket);

  socket.on("message", onMessage);
  socket.once("error", onError);
  console.log("eventNames:", socket.eventNames().map(String).sort().join(","));
} finally {
  socket.removeListener("message", onMessage);
  socket.removeListener("error", onError);
  await new Promise<void>((resolve) => socket.bind(0, "127.0.0.1", resolve));
  await new Promise<void>((resolve) => socket.close(resolve));
}
