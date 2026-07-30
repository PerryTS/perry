import * as net from "node:net";

const cases: [string, any][] = [
  ["host array", { port: 1, host: ["127.0.0.1"] }],
  ["host null byte", { port: 1, host: "127.0.0.1\0x" }],
  ["lookup", { port: 1, host: "localhost", lookup: true }],
  ["autoSelectFamily", { port: 1, autoSelectFamily: "yes" }],
  ["attempt timeout type", { port: 1, autoSelectFamilyAttemptTimeout: "10" }],
  ["attempt timeout range", { port: 1, autoSelectFamilyAttemptTimeout: 0 }],
  ["objectMode", { port: 1, objectMode: true }],
];

async function destroyAndSettle(socket: net.Socket): Promise<void> {
  await new Promise<void>((resolve) => {
    socket.once("error", () => resolve());
    socket.once("close", () => resolve());
    socket.destroy();
  });
}

for (const [label, options] of cases) {
  try {
    const socket = net.connect(options);
    await destroyAndSettle(socket);
    console.log(label, "OK");
  } catch (error: any) {
    console.log(label, error.name, error.code);
  }
}
