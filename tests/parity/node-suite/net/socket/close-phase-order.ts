// Sockets destroyed in one turn close in the close-callbacks phase, newest
// first (libuv prepends each closing handle), and each 'close' listener's
// ticks run before the next socket's 'close'.
import * as net from "node:net";

function watch(label: string, socket: net.Socket, log: string[]) {
  socket.on("error", () => {});
  socket.on("close", () => {
    log.push(`${label} close`);
    process.nextTick(() => log.push(`${label} tick`));
  });
}

function round(name: string, make: () => net.Socket[], next: () => void) {
  const log: string[] = [];
  const sockets = make();
  sockets.forEach((socket, i) => watch("ABC"[i], socket, log));
  let open = sockets.length;
  sockets.forEach((socket) => socket.once("close", () => --open === 0 && setTimeout(() => {
    console.log(name, log.join(", "));
    next();
  }, 20)));
  sockets.forEach((socket) => socket.destroy());
}

const server = net.createServer(() => {});
server.listen(0, "127.0.0.1", () => {
  const { port } = server.address() as net.AddressInfo;
  const connected = (done: (s: net.Socket) => void) => {
    const s = net.connect(port, "127.0.0.1", () => done(s));
  };
  round("resolving", () => [0, 1, 2].map(() => net.connect(1, "localhost")), () => {
    connected((a) => connected((b) => connected((c) => {
      round("connected", () => [a, b, c], () => {
        connected((b) => {
          round("mixed", () => [net.connect(1, "localhost"), b], () => server.close());
        });
      });
    })));
  });
});
