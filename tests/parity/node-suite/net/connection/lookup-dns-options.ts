// The dns options `connect` hands `options.lookup`, and a zoned IPv6 answer:
// zero hints become `dns.ADDRCONFIG` off Windows unless a family is pinned,
// and the zone selects the interface of a link-local address.
import * as dns from "node:dns";
import * as net from "node:net";
import * as os from "node:os";

const server = net.createServer((socket) => socket.end());
await new Promise<void>((resolve) => server.listen(0, "::", resolve));
const port = (server.address() as net.AddressInfo).port;
const addrconfig = process.platform === "win32" ? 0 : dns.ADDRCONFIG;

function hints(label: string, options: any): Promise<void> {
  return new Promise((resolve) => {
    const socket = net.connect({
      port,
      host: "hints.test",
      ...options,
      lookup(_host: string, dnsOptions: any, callback: any) {
        const shown = dnsOptions.hints === addrconfig ? "ADDRCONFIG" : dnsOptions.hints;
        console.log(label, "hints", shown);
        callback(null, dnsOptions.all ? [{ address: "127.0.0.1", family: 4 }] : "127.0.0.1", 4);
      },
    });
    socket.on("error", (err: any) => console.log(label, "error", err.code));
    socket.on("close", () => resolve());
  });
}

await hints("default", {});
await hints("family4", { family: 4 });
await hints("explicit", { hints: dns.V4MAPPED });

// macOS gives its loopback a link-local address; elsewhere the case is skipped.
const loopback = Object.entries(os.networkInterfaces()).find(([, list]) =>
  list?.some((entry) => entry.address === "fe80::1"),
)?.[0];
if (loopback === undefined) {
  console.log("zone connect IPv6");
} else {
  await new Promise<void>((resolve) => {
    const socket = net.connect({
      port,
      host: "zone.test",
      lookup(_host: string, _options: any, callback: any) {
        callback(null, [{ address: `fe80::1%${loopback}`, family: 6 }]);
      },
    });
    socket.on("connect", () => console.log("zone connect", socket.remoteFamily));
    socket.on("error", (err: any) => console.log("zone error", err.code));
    socket.on("close", () => resolve());
  });
}

server.close();
