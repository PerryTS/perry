// Issue #10908 — net.Socket.read() must follow the readable-stream pull
// contract used by undici's llhttp loop: return a Buffer while bytes are
// queued and `null` after the queue is drained. The initial call exercises
// the statically tagged Socket path; the readable callback uses an `any`
// alias, matching compiled npm code that reaches handle-method dispatch.

import { connect, createServer } from "node:net";

const server = createServer((socket: any) => {
  setTimeout(() => socket.end("hello"), 20);
});

server.listen(0, "127.0.0.1", () => {
  const address = server.address() as any;
  const client = connect(address.port, "127.0.0.1");
  const dynamicClient: any = client;
  let consumed = false;

  client.on("connect", () => {
    console.log("empty=" + (client.read() === null));
  });

  dynamicClient.on("readable", () => {
    if (consumed) return;
    const chunk = dynamicClient.read();
    if (chunk === null) return;
    consumed = true;
    console.log("buffer=" + Buffer.isBuffer(chunk));
    console.log("body=" + chunk.toString());
    console.log("drained=" + (dynamicClient.read() === null));
  });

  client.on("end", () => {
    console.log("end");
    server.close();
  });
});

setTimeout(() => {}, 1500);
