import { createServer, get } from "node:http";

const server = createServer((req, res) => {
  const acceptedSocket = req.socket;
  console.log("initial identity:", res.socket === acceptedSocket, res.connection === acceptedSocket);
  req.socket = 42 as any;
  console.log("request reassigned:", req.socket === 42);
  console.log("response identity:", res.socket === acceptedSocket, res.connection === acceptedSocket);
  res.end("ok");
});

server.listen(0, "127.0.0.1", () => {
  const { port } = server.address() as { port: number };
  get({ host: "127.0.0.1", port, agent: false }, (res) => {
    res.resume();
    res.on("end", () => server.close());
  });
});
