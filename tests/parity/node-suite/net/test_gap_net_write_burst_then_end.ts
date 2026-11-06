// #11106 — a client that bursts 40,000 small writes and then calls `end()`
// must deliver every byte before the FIN. Perry delivered 0: each write was
// its own event-loop operation, the 32,769th overflowed the loop's operation
// table (`write ENOMEM`), and the resulting destroy cancelled everything
// already queued. Every write callback must fire too, in order.
import net from "node:net";

const N = 40000;
const CHUNK = Buffer.alloc(64, 0x61);

const srv = net.createServer((s) => {
  let got = 0;
  s.on("data", (d) => {
    got += d.length;
  });
  s.on("end", () => {
    console.log("server received", got);
    srv.close();
  });
});

srv.listen(0, "127.0.0.1", () => {
  const c = net.connect((srv.address() as net.AddressInfo).port, "127.0.0.1", () => {
    let callbacks = 0;
    let errors = 0;
    let inOrder = true;
    for (let i = 0; i < N; i++) {
      const n = i;
      c.write(CHUNK, (err) => {
        if (err) errors++;
        if (n !== callbacks) inOrder = false;
        callbacks++;
      });
    }
    c.end(() => {
      console.log("callbacks", callbacks, "errors", errors, "in order", inOrder);
      console.log("bytesWritten", c.bytesWritten);
    });
  });
  c.on("error", (e) => console.log("client error", e.message));
  c.on("close", () => console.log("client close"));
});
