import * as net from "node:net";
import { createServer } from "node:net";

const keys = Object.keys(net);

function logExport(name: string, value: any) {
  console.log("export:", name, typeof value, value?.length, keys.includes(name));
}

logExport("createServer", net.createServer);
logExport("Server", (net as any).Server);
logExport("_normalizeArgs", (net as any)._normalizeArgs);
logExport("_createServerHandle", (net as any)._createServerHandle);

console.log("named createServer:", typeof createServer, createServer.length);
const server = createServer();
console.log("named server:", typeof server);
console.log("namespace server:", typeof net.createServer());
console.log("Server call:", typeof (net as any).Server());

const normalized = (net as any)._normalizeArgs([80, "localhost"]);
console.log("normalize port-host:", JSON.stringify(normalized[0]), normalized[1] === null);

const cb = () => {};
const normalizedWithCallback = (net as any)._normalizeArgs([80, "localhost", cb]);
console.log(
  "normalize callback:",
  JSON.stringify(normalizedWithCallback[0]),
  normalizedWithCallback[1] === cb,
);

const normalizedPath = (net as any)._normalizeArgs(["/tmp/perry.sock", cb]);
console.log("normalize path:", JSON.stringify(normalizedPath[0]), normalizedPath[1] === cb);
