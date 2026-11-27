// A TLS socket over an existing socket: destroying the wrapper closes the
// parent first, and both 'close' events run before either listener's ticks.
import * as net from "node:net";
import * as tls from "node:tls";

const server = net.createServer(() => {});
server.listen(0, "127.0.0.1", () => {
  const { port } = server.address() as net.AddressInfo;
  const parent = net.connect(port, "127.0.0.1", () => {
    const wrapper = tls.connect({ socket: parent, servername: "localhost" });
    wrapper.on("error", () => {});
    parent.on("close", () => {
      console.log("parent close");
      process.nextTick(() => console.log("parent tick"));
    });
    wrapper.on("close", () => {
      console.log("wrapper close");
      process.nextTick(() => console.log("wrapper tick"));
      server.close();
    });
    setImmediate(() => wrapper.destroy());
  });
});
