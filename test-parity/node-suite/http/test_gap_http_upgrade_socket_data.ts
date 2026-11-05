// N8 (net phase A, decision 77): an HTTP `'upgrade'` hands the listener the
// connection itself as an ordinary net.Socket. The upgrade is one route store
// on the connection, so the bytes that arrived with the head come back as
// `head`, and every later byte is the socket's own `data`, whether the
// listener's parameter is untyped, typed `stream.Duplex`, or passed on to a
// helper. A socket the compiler mis-tags as a ws Client never sees `data`
// and this test times out.
import * as http from "node:http";
import * as net from "node:net";
import type { Duplex } from "node:stream";

function echoUntil(socket: Duplex, head: Buffer, label: string, done: () => void) {
  let bytes = head.toString();
  socket.on("data", (part: Buffer) => {
    bytes += part.toString();
    if (bytes.endsWith("+TAIL")) {
      console.log(label, "bytes", bytes, "flowing", socket.readableFlowing);
      socket.end("DONE");
      done();
    }
  });
  socket.write("READY");
}

const server = http.createServer();
let upgrades = 0;
server.on("upgrade", (req: http.IncomingMessage, socket: Duplex, head: Buffer) => {
  upgrades++;
  console.log("upgrade", req.method, req.url, req.headers.upgrade, "head", head.length);
  if (req.url === "/typed") {
    let bytes = head.toString();
    socket.on("data", (part: Buffer) => {
      bytes += part.toString();
      if (bytes.endsWith("+TAIL")) {
        console.log("typed bytes", bytes, "listeners", socket.listenerCount("data"));
        socket.end("DONE");
      }
    });
    socket.write("READY");
  } else {
    echoUntil(socket, head, "helper", () => {});
  }
});

function client(path: string): Promise<string> {
  return new Promise((resolve) => {
    const port = (server.address() as net.AddressInfo).port;
    const socket = net.connect(port, "127.0.0.1");
    let output = "";
    let sent = false;
    socket.on("connect", () =>
      socket.write(
        "GET " + path + " HTTP/1.1\r\nHost: localhost\r\nConnection: Upgrade\r\nUpgrade: echo\r\n\r\nHEAD+SAME-PACKET",
      ),
    );
    socket.on("data", (part: Buffer) => {
      output += part.toString();
      if (!sent && output.includes("READY")) {
        sent = true;
        socket.write("+TAIL");
      }
    });
    socket.on("end", () => socket.end());
    socket.on("close", () => resolve(output));
  });
}

server.listen(0, "127.0.0.1", async () => {
  console.log("client typed", await client("/typed"));
  console.log("client helper", await client("/helper"));
  server.close(() => console.log("closed upgrades", upgrades));
});
