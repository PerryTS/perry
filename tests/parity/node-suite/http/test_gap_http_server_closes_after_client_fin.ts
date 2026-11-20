// #11452: an http server hit by a client that sends one keep-alive request and
// then closes its end (what `curl` does) kept one socket per connection open
// forever. Node closes the server socket once the peer's FIN arrives.
import * as http from "node:http";
import * as net from "node:net";

let accepted = 0;
let closed = 0;
const server = http.createServer((req, res) => res.end("hi"));
server.on("connection", (socket) => {
  accepted++;
  socket.on("close", () => closed++);
});

function oneShot(port: number, close: boolean): Promise<boolean> {
  return new Promise((ok, bad) => {
    const s = net.connect(port, "127.0.0.1", () => {
      s.write(
        "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: " +
          (close ? "close" : "keep-alive") +
          "\r\n\r\n",
      );
    });
    let got = "";
    s.on("data", (d) => {
      got += d;
      // A keep-alive response: once the body is in, the client goes away.
      if (got.endsWith("hi")) s.end();
    });
    s.on("error", bad);
    s.on("close", () => ok(got.startsWith("HTTP/1.1 200") && got.endsWith("hi")));
  });
}

server.listen(0, "127.0.0.1", async () => {
  const port = (server.address() as any).port;
  const N = 200;
  let ok = 0;
  for (let i = 0; i < N; i++) if (await oneShot(port, i % 2 === 1)) ok++;
  await new Promise((r) => setTimeout(r, 200));
  console.log("responses", ok === N);
  console.log("accepted", accepted === N);
  console.log("every server socket closed", accepted > 0 && closed === accepted);
  server.close(() => console.log("server closed"));
});
