// `http.Server` emits `'connection'` with the accepted socket, before any
// request on it. Libraries rely on that argument: `server-destroy` (used by
// Astro's node adapter) keys open connections by
// `conn.remoteAddress + ":" + conn.remotePort` and drops them on the
// socket's `'close'`.
import { createServer, get } from "node:http";

const connections: Record<string, any> = {};
const seen: any[] = [];
const events: string[] = [];
let requests = 0;

const server = createServer((req, res) => {
  events.push("request");
  requests++;
  console.log("req.socket is the connection socket:", req.socket === seen[requests - 1]);
  console.log("res.socket is req.socket:", res.socket === req.socket);
  console.log("res.connection is req.socket:", res.connection === req.socket);
  res.end("ok");
});

server.on("connection", (conn: any) => {
  events.push("connection");
  seen.push(conn);
  console.log("connection arg type:", typeof conn);
  console.log("remoteAddress type:", typeof conn?.remoteAddress);
  console.log("remotePort type:", typeof conn?.remotePort);
  console.log("has on/destroy:", typeof conn?.on, typeof conn?.destroy);

  const key = conn.remoteAddress + ":" + conn.remotePort;
  connections[key] = conn;
  console.log("tracked after connect:", Object.keys(connections).length);
  conn.on("close", () => {
    events.push("socket-close");
    delete connections[key];
  });
});

server.listen(0, "127.0.0.1", () => {
  const { port } = server.address() as { port: number };
  const request = () => {
    get({ host: "127.0.0.1", port, path: "/", agent: false }, (res) => {
      res.resume();
      res.on("end", () => {
        if (requests < 2) {
          request();
        } else {
          server.close(() => {
            console.log("connection events:", seen.length);
            console.log("tracked after close:", Object.keys(connections).length);
            console.log("order:", events.join(" > "));
          });
        }
      });
    });
  };
  request();
});
