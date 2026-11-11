// An outbound connect to a name whose lookup takes 400 ms must not stop the
// loop: the 25 ms timer keeps ticking and a client that is already connected
// is still echoed.
import * as net from "node:net";

const peerPort = Number(process.env.PEER_PORT);
const server = net.createServer((socket) => {
  socket.on("data", (chunk) => socket.write(chunk));
  socket.on("end", () => socket.end());
  socket.on("close", () => console.log("client closed"));
});
server.listen(0, () => {
  console.log("listening");
  let ticks = 0;
  const timer = setInterval(() => ticks++, 25);
  const out = net.connect(peerPort, "slow.test");
  console.log("dialing");
  out.on("connect", () => {
    console.log("connected", ticks >= 10 ? "ticks ok" : `too few ticks (${ticks})`);
    clearInterval(timer);
    out.end("bye");
  });
  out.on("error", (error: any) => console.log("error", error.code));
  out.on("close", () => {
    console.log("closed");
    server.close();
  });
});
