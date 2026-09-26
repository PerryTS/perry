import * as net from "node:net";
const server = net.createServer((sock) => {
  sock.on("data", (d) => sock.end("echo:" + d.toString()));
});
server.listen(0, () => {
  const port = (server.address() as net.AddressInfo).port;
  const c = net.connect(port, "127.0.0.1", () => c.write("ping"));
  let got = "";
  c.on("data", (d) => (got += d.toString()));
  c.on("close", () => {
    console.log("net client got:", got);
    server.close(() => console.log("net server closed"));
  });
});
