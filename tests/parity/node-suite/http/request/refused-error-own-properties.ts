// A refused request's 'error' carries Node's system-error fields as ordinary
// own properties. The port is ephemeral, so it is printed as a placeholder.
import * as http from "node:http";
import * as net from "node:net";

const server = net.createServer();
await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
const port = (server.address() as net.AddressInfo).port;
await new Promise<void>((resolve) => server.close(() => resolve()));

const error: any = await new Promise((resolve) => {
  http.get({ host: "127.0.0.1", port }).once("error", resolve);
});
console.log(error instanceof Error, Object.keys(error).join(","));
console.log(error.code, error.syscall, error.address, error.port === port, typeof error.errno);
console.log(JSON.stringify(error).replace(String(port), "<port>"));
