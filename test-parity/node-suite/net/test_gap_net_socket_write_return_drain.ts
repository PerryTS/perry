// #11111 — `net.Socket#write()` returns Node's boolean: `true` while
// `writableLength` stays under `writableHighWaterMark`, `false` once a chunk
// reaches it, and a `false` is followed by exactly one `'drain'` (emitted
// before the completed write's callback). Drain-aware writers — mongodb's
// `Connection.writeCommand` does `if (socket.write(buf)) return; await
// once(socket, "drain")` — hung forever on Perry's `undefined`.
//
// The `false` cases write BEFORE the connection completes, where Node holds
// every byte in the stream. Once connected, Node writes to the kernel
// synchronously when it can, so whether a large connected write returns
// `false` depends on the host's socket buffers — not printed here.
import net from "node:net";
import { once } from "node:events";

let clients = 2;
const server = net.createServer((sock) => {
  let got = 0;
  sock.on("data", (d) => {
    got += d.length;
  });
  sock.on("end", () => {
    console.log("server received", got);
    sock.end();
  });
});

function closed() {
  if (--clients === 0) server.close();
}

async function drainAware(sock: any, chunk: Buffer): Promise<string> {
  // The mongodb shape, on an untyped receiver.
  if (sock.write(chunk)) return "no wait";
  await once(sock, "drain");
  return "waited for drain";
}

function typedClient(port: number): Promise<void> {
  return new Promise((resolve) => {
    const client = net.connect(port, "127.0.0.1");
    const order: string[] = [];
    let drains = 0;
    client.on("drain", () => {
      drains++;
      order.push("drain");
    });
    client.on("close", () => {
      closed();
      resolve();
    });
    const hwm = client.writableHighWaterMark;
    console.log("writableHighWaterMark", hwm);
    console.log("needDrain before", client.writableNeedDrain);

    const small = client.write("hello");
    console.log("small write", typeof small, small, "length", client.writableLength);

    const big = Buffer.alloc(hwm * 4, 0x62);
    const r = client.write(big, (err) => order.push("big cb " + (err ?? "ok")));
    console.log("big write", typeof r, r, "length", client.writableLength);
    console.log("needDrain after big", client.writableNeedDrain);

    client.once("drain", async () => {
      console.log("in drain: needDrain", client.writableNeedDrain, "length", client.writableLength);
      await new Promise((r) => setImmediate(r));
      console.log("order", order.join(","));
      const again = client.write("x");
      console.log("connected small write", typeof again, again);
      await new Promise((r) => setImmediate(r));
      console.log("drains", drains);
      client.end();
    });
  });
}

async function untypedClient(port: number): Promise<void> {
  const sock: any = net.connect({ host: "127.0.0.1", port });
  sock.on("close", closed);
  const hwm = sock.writableHighWaterMark;
  console.log("untyped:", await drainAware(sock, Buffer.alloc(hwm, 0x63)));
  console.log("untyped needDrain", sock.writableNeedDrain, "length", sock.writableLength);
  console.log("untyped:", await drainAware(sock, Buffer.alloc(16, 0x64)));
  sock.end();
}

server.listen(0, "127.0.0.1", async () => {
  const port = (server.address() as net.AddressInfo).port;
  await typedClient(port);
  await untypedClient(port);
});
