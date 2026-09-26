import { WebSocketServer, WebSocket } from "ws";
const wss = new WebSocketServer({ port: 0 }, () => {
  const port = (wss.address() as any).port;
  const ws = new WebSocket("ws://127.0.0.1:" + port);
  ws.on("open", () => ws.send("hello"));
  ws.on("message", (m: any) => {
    console.log("ws client got:", m.toString());
    ws.close();
  });
  ws.on("close", () => {
    console.log("ws client closed");
    wss.close(() => console.log("ws server closed"));
  });
});
wss.on("connection", (s: any) => s.on("message", (m: any) => s.send("echo:" + m.toString())));
