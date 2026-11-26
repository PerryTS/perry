// The 'error' a socket emits is an `Error`, for a syscall failure and for a
// coded stream error alike, with Node's own properties in Node's order.
import * as net from "node:net";

function describe(label: string, error: any): void {
  console.log(label, error instanceof Error, Object.prototype.toString.call(error), error.name);
  console.log(Object.keys(error).join(","));
}

const server = net.createServer();
await new Promise<void>((resolve, reject) => {
  server.once("error", reject);
  server.listen(0, "127.0.0.1", resolve);
});
const port = (server.address() as net.AddressInfo).port;
await new Promise<void>((resolve) => server.close(() => resolve()));

const refused: any = await new Promise((resolve) => {
  net.connect(port, "127.0.0.1").once("error", resolve);
});
describe("refused", refused);
console.log(String(refused.stack).split("\n")[0].replace(`:${port}`, ":<port>"));

const socket = new net.Socket();
socket.destroy();
const destroyed: any = await new Promise((resolve) => socket.write("x", resolve));
describe("destroyed", destroyed);
