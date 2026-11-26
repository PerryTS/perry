// The system resolver's answer is reported through 'lookup' events, as a
// custom `options.lookup` answer is. Which addresses `localhost` has depends
// on the host, so the case checks the IPv4 one and the event shape.
import * as net from "node:net";

const server = net.createServer((socket) => socket.end());
await new Promise<void>((resolve) => server.listen(0, resolve));
const port = (server.address() as net.AddressInfo).port;

function attempt(label: string, options: any): Promise<void> {
  return new Promise((resolve) => {
    const events: any[] = [];
    const socket = net.connect({ port, ...options });
    socket.on("lookup", (err: any, address: any, family: any, host: any) => {
      events.push({ err, address, family, host });
    });
    socket.on("connect", () => console.log(label, "connect"));
    socket.on("error", (err: any) => console.log(label, "error", err.code, events[0]?.err === err));
    socket.on("close", () => {
      console.log(label, "events", events.length > 0);
      console.log(label, "ipv4", events.some((e) => e.address === "127.0.0.1" && e.family === 4));
      console.log(label, "shape", events.every((e) => e.host === options.host && (e.err === null || e.err?.code === "ENOTFOUND")));
      resolve();
    });
  });
}

await attempt("localhost", { host: "localhost" });
await attempt("family4", { host: "localhost", family: 4 });
await attempt("missing", { host: "no-such-host.invalid" });
await attempt("ip", { host: "127.0.0.1" });

const destroyed = net.connect({ port, host: "localhost" });
destroyed.on("lookup", () => {
  console.log("destroyed lookup");
  destroyed.destroy();
});
destroyed.on("connect", () => console.log("destroyed connect"));
await new Promise((resolve) => destroyed.on("close", resolve));
console.log("destroyed close");

server.close();
