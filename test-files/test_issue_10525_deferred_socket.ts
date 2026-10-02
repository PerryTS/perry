import * as net from "node:net";

const variants = ["direct", "queueMicrotask", "promise", "nextTick", "setImmediate"];
let variantIndex = 0;
function run(): void {
  const variant = variants[variantIndex];
  const server = net.createServer((peer) => {
    peer.setNoDelay(true);
    peer.on("data", (chunk) => peer.write(chunk));
  });
  server.listen(0, "127.0.0.1", () => {
    const socket = net.connect((server.address() as net.AddressInfo).port, "127.0.0.1");
    let replies = 0;
    let bytes = 0;
    const send = () => socket.write("ping");
    socket.on("data", (chunk) => {
      bytes += chunk.length;
      if (++replies === 16) {
        console.log(variant, replies, bytes);
        socket.destroy();
        server.close(() => {
          if (++variantIndex < variants.length) run();
        });
      } else if (variant === "direct") send();
      else if (variant === "queueMicrotask") queueMicrotask(send);
      else if (variant === "promise") Promise.resolve().then(send);
      else if (variant === "nextTick") process.nextTick(send);
      else setImmediate(send);
    });
    socket.on("connect", send);
  });
}
run();
