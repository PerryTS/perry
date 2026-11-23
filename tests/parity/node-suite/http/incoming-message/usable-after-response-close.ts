// The request stays a readable object after its response closed (#15172): its
// fields, its stream state and its own properties, in the response's 'close'
// and on a later timer.
import http from "node:http";
import net from "node:net";

const inspect = (when: string, req: http.IncomingMessage) => {
  let write: string;
  try {
    (req as any).drained = true;
    write = "ok";
  } catch (error) {
    write = `throws ${String(error)}`;
  }
  console.log(`${when}: method=${req.method} readableEnded=${req.readableEnded} destroyed=${req.destroyed} readable=${req.readable} own=${(req as any).drained}; set ${write}`);
};

const server = http.createServer((req, res) => {
  inspect("handler", req);
  res.on("close", () => {
    inspect("res 'close'", req);
    setTimeout(() => {
      inspect("timer after 'close'", req);
      server.close();
    });
  });
  res.end("ok");
});

server.listen(0, "127.0.0.1", () => {
  const { port } = server.address() as { port: number };
  const client = net.connect(port, "127.0.0.1", () => {
    client.end("POST / HTTP/1.1\r\nHost: x\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}");
  });
  client.on("data", () => {});
});
