// A name that resolves after a delay, then an ordinary connection: the program
// sends a request and prints the peer's answer.
import * as net from "node:net";

const socket = net.connect(Number(process.env.PEER_PORT), "echo.test");
socket.on("connect", () => socket.write("ping"));
socket.on("data", (chunk) => {
  console.log("got", String(chunk));
  socket.end();
});
socket.on("error", (error: any) => console.log("error", error.code));
socket.on("close", () => console.log("closed"));
