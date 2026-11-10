// A peer's `end("x")` delivers data and EOF together. Node reads them in two
// polls, so a `setImmediate` queued by the server's 'data' handler runs before
// 'end' and before the allowHalfOpen=false auto-end; its `socket.end(x)` then
// succeeds instead of failing with ERR_STREAM_WRITE_AFTER_END.
import { connect, createServer, type Socket } from "node:net";

const ROUNDS = 8;

function round(port: number, id: string): Promise<string[]> {
  return new Promise((resolve, reject) => {
    const log: string[] = [];
    let body = "";
    let open = 2;
    const closed = () => --open === 0 && resolve(log);
    const client = connect(port, "127.0.0.1", () => client.end(id));
    client.on("data", (chunk) => (body += String(chunk)));
    client.on("error", reject);
    client.on("close", () => {
      log.push("client-body:" + body);
      closed();
    });
    pending.set(id, { log, closed });
  });
}

const pending = new Map<string, { log: string[]; closed: () => void }>();
const sockets = new Set<Socket>();
const server = createServer((socket) => {
  sockets.add(socket);
  let log: string[] = [];
  let closed = () => {};
  socket.on("close", () => {
    sockets.delete(socket);
    closed();
  });
  socket.on("end", () => log.push("srv-end"));
  socket.on("error", (e) => log.push("srv-error:" + (e as any).code));
  socket.once("data", (chunk) => {
    const id = String(chunk);
    ({ log, closed } = pending.get(id)!);
    log.push("srv-data");
    socket.write(id);
    setImmediate(() => {
      log.push("srv-immediate");
      socket.end(id);
    });
  });
});

try {
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const port = (server.address() as any).port;
  for (let i = 0; i < ROUNDS; i++) {
    const id = String.fromCharCode(97 + i);
    const log = await round(port, id);
    const errors = log.filter((line) => line.startsWith("srv-error"));
    const immediate = log.indexOf("srv-immediate");
    const end = log.indexOf("srv-end");
    console.log(
      `round ${id}:`,
      log[0],
      immediate >= 0 && end > immediate ? "immediate-before-end" : "end-first",
      errors.length === 0 ? "no-error" : errors.join(","),
      log.find((line) => line.startsWith("client-body")),
    );
  }
} finally {
  for (const socket of sockets) socket.destroy();
  if (server.listening) {
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
}
