// #11452: a WebSocket carried by an http server ends with our FIN and the
// peer's. The server closed its descriptor on neither — once the close
// handshake had finished, the peer's FIN found the connection already
// `closing` and was dropped — so every WebSocket connection kept a socket for
// the life of the process.
//
// Deterministic facts only: every accepted connection emits 'close'.
import http from "node:http";
import { WebSocket, WebSocketServer } from "ws";

let accepted = 0;
let closed = 0;
const server = http.createServer((req, res) => res.end("plain"));
server.on("connection", (socket: any) => {
  accepted++;
  socket.on("close", () => closed++);
});
const wss = new WebSocketServer({ server });
wss.on("connection", (ws: any) => {
  ws.on("message", (m: any) => ws.send("echo:" + m));
});

function cycle(port: number, serverCloses: boolean): Promise<boolean> {
  return new Promise((ok) => {
    const ws: any = new WebSocket("ws://127.0.0.1:" + port + "/");
    let echoed = false;
    ws.on("open", () => ws.send("x"));
    ws.on("message", (m: any) => {
      echoed = String(m) === "echo:x";
      if (serverCloses) {
        for (const c of wss.clients) (c as any).close(1000, "done");
      } else {
        ws.close(1000, "done");
      }
    });
    ws.on("error", () => {});
    ws.on("close", (code: number) => ok(echoed && code === 1000));
  });
}

server.listen(0, "127.0.0.1", async () => {
  const port = (server.address() as any).port;
  const N = 40;
  let ok = 0;
  for (let i = 0; i < N; i++) if (await cycle(port, i % 2 === 1)) ok++;
  await new Promise((r) => setTimeout(r, 300));
  console.log("cycles", ok === N);
  console.log("accepted", accepted === N);
  console.log("every server socket closed", accepted > 0 && closed === accepted);
  wss.close();
  server.close(() => console.log("server closed"));
});
