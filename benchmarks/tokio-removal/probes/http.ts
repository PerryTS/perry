import * as http from "node:http";
const server = http.createServer((req, res) => res.end("hi"));
server.listen(0, () => {
  const port = (server.address() as any).port;
  http.get("http://127.0.0.1:" + port + "/", (res) => {
    let body = "";
    res.on("data", (d) => (body += d));
    res.on("end", () => {
      console.log("http got:", body);
      server.close(() => console.log("http server closed"));
    });
  });
});
