import * as http2 from "node:http2";

// process.exit() runs no 'close' listener of a session it abandons.
const server = http2.createServer();
server.listen(0, "127.0.0.1", () => {
  const client = http2.connect(`http://127.0.0.1:${(server.address() as any).port}`);
  client.on("close", () => console.log("client close"));
  client.on("connect", () => {
    console.log("connected");
    process.exit(0);
  });
});
