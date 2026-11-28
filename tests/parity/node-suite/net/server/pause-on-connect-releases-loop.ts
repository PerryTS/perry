import net from "node:net";

// A pauseOnConnect socket never reads, so once the listener closes nothing holds the loop.
const reached = setTimeout(() => {
  console.log("still alive");
  process.exit(3);
}, 15000);
reached.unref();

const server = net.createServer({ pauseOnConnect: true }, () => {
  console.log("accepted");
  server.close();
});
server.listen(0, "127.0.0.1", () => {
  const client = net.connect((server.address() as net.AddressInfo).port, "127.0.0.1");
  client.unref();
});
process.on("exit", (code) => console.log("exit", code));
