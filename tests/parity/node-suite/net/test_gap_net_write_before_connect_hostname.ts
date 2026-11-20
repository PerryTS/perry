// Writes issued before `connect` completes — including while a hostname is
// still resolving — belong to the connection that is finally established.
// Perry answered a write on a still-resolving `localhost` socket with
// `write ENOENT` and then hung; an IP literal worked. Found alongside #11106.
import net from "node:net";

// The two connections race each other; print in a fixed order.
const received: string[] = [];
const srv = net.createServer((s) => {
  let got = "";
  s.on("data", (d) => {
    got += d.toString();
  });
  s.on("end", () => {
    received.push("server got " + JSON.stringify(got));
    s.end();
  });
});

const results: string[] = [];
let pending = 2;
srv.listen(0, "127.0.0.1", () => {
  const port = (srv.address() as net.AddressInfo).port;
  for (const host of ["127.0.0.1", "localhost"]) {
    const c = net.connect(port, host);
    c.write(host + ":one,");
    const r = c.write(host + ":two");
    results.push(host + " early write " + r + " pending " + c.writableLength);
    // `end()` from 'connect', not before it: Node 26 itself drops the FIN of
    // an `end()` issued while a hostname connect is still selecting a family.
    c.on("connect", () => c.end());
    c.on("error", (e) => console.log(host, "error", e.message));
    c.on("close", () => {
      if (--pending === 0) {
        console.log(received.sort().join("\n"));
        console.log(results.join("\n"));
        srv.close();
      }
    });
  }
});
