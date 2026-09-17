// #10428 / #10429: `node:net` and `node:http` exports reached as VALUES.
//
// A direct call on an import binding (`net.connect(port, host)`) lowers through
// codegen's static native table. Every other shape — a module object aliased or
// passed around, a destructured or pulled-out export, `new` on a bound class
// value, and all CommonJS `require('net')` forms that pg / mysql2 / ioredis / ws
// use when compiled from source — reaches the runtime's module-object dispatch.
// That dispatch forwarded to a provider callback registered only when the
// stdlib was auto-optimized for an http import, so under
// `PERRY_NO_AUTO_OPTIMIZE=1`, or in any net-only program, `connect()` /
// `createConnection()` / `http.request()` returned `undefined`. `isIP` and the
// class constructors were never forwarded at all, and
// `require('node:http') !== require('http')`.
//
// Everything runs against in-process servers on 127.0.0.1 (port 0).
import * as net from "node:net";
import * as http from "node:http";
import * as cjs from "./gap_10428_10429_module_values_helper.cjs";

const HOST = "127.0.0.1";
const n: any = net;
const h: any = http;

function kind(v: any): string {
  if (v === undefined) return "undefined";
  return `${typeof v} on:${typeof v.on} write:${typeof v.write}`;
}

// ── synchronous value reads ──
function check(label: string, read: () => unknown[]): void {
  try {
    console.log(`${label}:`, ...read());
  } catch (e: any) {
    console.log(`${label}: threw ${e?.name}`);
  }
}
check("alias typeof connect/isIP", () => [typeof n.connect, typeof n.isIP]);
check("alias isIP v4/v6/bad", () => [n.isIP("127.0.0.1"), n.isIP("::1"), n.isIP("nope")]);
check("destructured isIPv4/isIPv6", () => {
  const { isIPv4, isIPv6 } = n;
  return [isIPv4("10.0.0.1"), isIPv6("10.0.0.1")];
});
check("cjs net.isIP", () => [cjs.isIP("::1"), cjs.isIP("10.1.2.3")]);
check("cjs require('node:x') === require('x')", () => [cjs.sameModules().join(",")]);

// ── sockets opened through each value shape, echoed by an in-process server ──
type Opener = (port: number) => any;
const openers: Array<[string, Opener]> = [
  ["alias n.connect(port, host)", (port) => n.connect(port, HOST)],
  [
    "destructured createConnection({ port, host })",
    (port) => {
      const { createConnection } = n;
      return createConnection({ port, host: HOST });
    },
  ],
  [
    "new (const Sock = n.Socket)().connect",
    (port) => {
      const Sock = n.Socket;
      const s = new Sock();
      s.connect(port, HOST);
      return s;
    },
  ],
  ["cjs Net.connect(port, host)", (port) => cjs.connectMember(port, HOST)],
  ["cjs (0, net.createConnection)({ port, host })", (port) => cjs.createConnectionCall({ port, host: HOST })],
  [
    "cjs fn-local new n2.Socket().connect",
    (port) => {
      const s = cjs.newLocalSocket();
      s.connect(port, HOST);
      return s;
    },
  ],
];

const echo = net.createServer((sock: any) => {
  sock.on("data", (d: any) => sock.write("echo:" + d.toString()));
});

function runSockets(port: number, index: number, done: () => void): void {
  if (index >= openers.length) {
    done();
    return;
  }
  const [label, open] = openers[index];
  const sock = open(port);
  console.log(`${label}: ${kind(sock)}`);
  if (sock === undefined || typeof sock.on !== "function") {
    runSockets(port, index + 1, done);
    return;
  }
  let finished = false;
  const next = () => {
    if (finished) return;
    finished = true;
    sock.destroy();
    runSockets(port, index + 1, done);
  };
  sock.on("connect", () => sock.write("#" + index));
  sock.on("data", (d: any) => {
    console.log(`  round trip: ${d.toString()}`);
    next();
  });
  sock.on("error", (e: any) => {
    console.log(`  socket error: ${e.message}`);
    next();
  });
}

// ── http.createServer / http.request reached as values (fastify, ws) ──
const web = cjs.createServer({}, (req: any, res: any) => {
  res.end("hi " + req.url);
});
console.log(`cjs http.createServer(options, handler): ${typeof web} listen:${typeof web?.listen}`);

function get(label: string, request: (opts: any, cb: (res: any) => void) => any, port: number, path: string, done: () => void): void {
  const req = request({ host: HOST, port, path }, (res: any) => {
    let body = "";
    res.on("data", (c: any) => {
      body += c.toString();
    });
    res.on("end", () => {
      console.log(`  ${res.statusCode} ${body}`);
      done();
    });
  });
  console.log(`${label}: ${kind(req)}`);
  if (req === undefined) {
    done();
    return;
  }
  req.on("error", (e: any) => {
    console.log(`  request error: ${e.message}`);
    done();
  });
  req.end();
}

// Fail with a diff instead of hanging if a round trip never completes.
const guard = setTimeout(() => {
  console.log("timed out");
  process.exit(1);
}, 10000);

echo.listen(0, HOST, () => {
  const echoPort = (echo.address() as any).port;
  runSockets(echoPort, 0, () => {
    echo.close();
    web.listen(0, HOST, () => {
      const webPort = (web.address() as any).port;
      const request = h.request;
      get("alias const request = h.request", request, webPort, "/alias", () => {
        get("cjs const request = http.request", cjs.request, webPort, "/cjs", () => {
          web.close();
          clearTimeout(guard);
          console.log("done");
        });
      });
    });
  });
});
