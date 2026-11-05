// CodeRabbit on #11130: every `end(cb)` callback runs, in call order — on a
// socket still connecting and on a connected one. Perry kept only one pending
// shutdown token, so the first callback was replaced and ran late (at close),
// after the second.
import net from "node:net";

const srv = net.createServer((s) => {
  s.resume();
  s.on("end", () => s.end());
});

srv.listen(0, "127.0.0.1", () => {
  const port = (srv.address() as net.AddressInfo).port;
  const a = net.connect(port, "127.0.0.1");
  a.write("x");
  a.end(() => console.log("connecting: end cb1"));
  a.end(() => console.log("connecting: end cb2"));
  a.on("error", (e) => console.log("a error", e.message));
  a.on("close", () => {
    const b = net.connect(port, "127.0.0.1", () => {
      b.end(() => console.log("connected: end cb1"));
      b.end(() => console.log("connected: end cb2"));
    });
    b.on("error", (e) => console.log("b error", e.message));
    b.on("close", () => {
      console.log("done");
      srv.close();
    });
  });
});
