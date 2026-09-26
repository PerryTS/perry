// Minimal node:http server for load generation: argv[2] = port.
import * as http from "node:http";
const port = Number(process.argv[2] || 18080);
const server = http.createServer((req, res) => {
  res.setHeader("content-type", "text/plain");
  res.end("hello world");
});
server.listen(port, "127.0.0.1", () => console.log("listening " + port));
