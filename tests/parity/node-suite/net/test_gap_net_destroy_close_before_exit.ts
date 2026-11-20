// #11227: `socket.destroy()` must still deliver 'close' when nothing else
// keeps the process alive. Node emits it on a nextTick. Perry marked the
// socket destroyed at once, so the keepalive gate saw no active handle and
// the program exited before the driver reported the close. undici's
// `Client.close()` awaits exactly that 'close', so it never settled.
import * as net from "node:net";
const server = net.createServer();
server.listen(0, "127.0.0.1", () => {
  const port = (server.address() as any).port;
  server.close(() => {
    // Nothing listens on `port` any more and nothing else is pending.
    const sock = net.connect(port, "127.0.0.1");
    sock.on("error", () => {});
    sock.on("close", function (this: any) {
      console.log("close fired, this is socket:", this === sock);
    });
    Promise.resolve().then(() => {
      sock.destroy();
      console.log("destroy called");
    });
  });
});
process.on("exit", () => console.log("exit"));
