// `options.lookup` replaces the resolver for a hostname: it receives the
// dns options, its answer is reported through `'lookup'` events, and the
// socket connects to the address it returns.
import * as net from "node:net";

const server = net.createServer((socket) => socket.end("hi"));
await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
const port = (server.address() as net.AddressInfo).port;

function attempt(label: string, options: any): Promise<void> {
  return new Promise((resolve) => {
    const socket = net.connect({ port, host: "custom.test", ...options });
    socket.on("lookup", (err: any, address: any, family: any, host: any) => {
      console.log(label, "lookup event", err?.message ?? err, address, family, host);
    });
    socket.on("connect", () => console.log(label, "connect", socket.remoteAddress));
    socket.on("data", (data) => console.log(label, "data", String(data)));
    socket.on("error", (err: any) => {
      console.log(label, "error", err.code, err.message.replace(`:${port}`, ":<port>"));
    });
    socket.on("close", () => resolve());
  });
}

await attempt("all", {
  lookup(host: string, options: any, callback: any) {
    console.log("all lookup", host, options.family, options.all);
    setTimeout(() => callback(null, [
      { address: "127.0.0.1", family: 4 },
      { address: "127.0.0.1", family: 4 },
    ]), 1);
  },
});

// A synchronous answer arrives before the caller can listen for 'lookup'.
await attempt("sync", {
  lookup(host: string, options: any, callback: any) {
    console.log("sync lookup", host);
    callback(null, options.all ? [{ address: "127.0.0.1", family: 4 }] : "127.0.0.1", 4);
  },
});

await attempt("family4", {
  family: 4,
  lookup(host: string, options: any, callback: any) {
    console.log("family4 lookup", host, options.family, options.all);
    setTimeout(() => callback(null, "127.0.0.1", 4), 1);
  },
});

await attempt("failed", {
  lookup(_host: string, _options: any, callback: any) {
    const err: any = new Error("no such name");
    err.code = "ENOTFOUND";
    setTimeout(() => callback(err), 1);
  },
});

await attempt("bad-ip", {
  family: 4,
  lookup(_host: string, _options: any, callback: any) {
    setTimeout(() => callback(null, "not-an-ip", 4), 1);
  },
});

await attempt("bad-family", {
  family: 4,
  lookup(_host: string, _options: any, callback: any) {
    setTimeout(() => callback(null, "127.0.0.1", 5), 1);
  },
});

// The server listens on IPv4 only, so the refused IPv6 answer falls back.
await attempt("fallback", {
  lookup(_host: string, _options: any, callback: any) {
    callback(null, [
      { address: "::1", family: 6 },
      { address: "127.0.0.1", family: 4 },
    ]);
  },
});

const destroyed = net.connect({
  port,
  host: "custom.test",
  lookup(_host: string, _options: any, callback: any) {
    setTimeout(() => callback(null, [{ address: "127.0.0.1", family: 4 }]), 1);
  },
});
destroyed.on("lookup", () => console.log("destroyed lookup event"));
destroyed.on("connect", () => console.log("destroyed connect"));
destroyed.destroy();
await new Promise((resolve) => setTimeout(resolve, 20));
console.log("destroyed", destroyed.destroyed, destroyed.connecting);

await attempt("ip-host", {
  host: "127.0.0.1",
  lookup() {
    console.log("ip-host lookup must not run");
  },
});

try {
  net.connect({ port, host: "custom.test", lookup: 1 as any });
} catch (err: any) {
  console.log("invalid", err.code, err.message);
}

server.close();
