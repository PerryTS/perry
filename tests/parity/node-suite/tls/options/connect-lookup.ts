// `tls.connect` resolves its host through `options.lookup`, with the same
// dns options and 'lookup' events as `net.connect`. The peer is a plain TCP
// server that hangs up, so only the transport's resolution is observed.
import * as net from "node:net";
import * as tls from "node:tls";

const server = net.createServer((socket) => socket.destroy());
await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
const port = (server.address() as net.AddressInfo).port;

for (const [label, options] of [
  ["options", { port, host: "tls-lookup.test" }],
  ["family4", { port, host: "tls-lookup.test", family: 4 }],
] as const) {
  await new Promise<void>((resolve) => {
    const socket = tls.connect({
      ...options,
      rejectUnauthorized: false,
      lookup(host: string, dnsOptions: any, callback: any) {
        console.log(label, "lookup", host, dnsOptions.family, dnsOptions.all);
        setTimeout(() => {
          callback(null, dnsOptions.all ? [{ address: "127.0.0.1", family: 4 }] : "127.0.0.1", 4);
        }, 1);
      },
    });
    socket.on("lookup", (err: any, address: any, family: any, host: any) => {
      console.log(label, "lookup event", err, address, family, host);
    });
    socket.on("error", () => console.log(label, "error"));
    socket.on("close", () => {
      console.log(label, "close");
      resolve();
    });
  });
}

server.close();
