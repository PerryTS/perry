// A server that destroys its connections and closes the server from the
// response's 'finish' listener (a shutdown endpoint) must still deliver the
// response: 'finish' fires once the bytes are handed to the OS, then 'close'.
import * as http from "node:http";

function run(name: string, status: number, body: string): Promise<void> {
  return new Promise((done) => {
    const events: string[] = [];
    const sockets = new Set<any>();
    const server = http.createServer((req, res) => {
      req.resume();
      res.on("finish", () => {
        events.push("finish");
        for (const socket of sockets) socket.destroy();
        server.close();
      });
      res.on("close", () => events.push("close"));
      res.writeHead(status);
      if (body) res.end(body);
      else res.end();
    });
    server.on("connection", (socket) => sockets.add(socket));
    server.listen(0, "127.0.0.1", () => {
      const port = (server.address() as any).port;
      const r = http.request(
        { host: "127.0.0.1", port, method: "POST", path: "/shutdown", agent: false },
        (res) => {
          let got = "";
          res.setEncoding("utf8");
          res.on("data", (d) => (got += d));
          res.on("end", () => {
            console.log(name, "client status", res.statusCode, JSON.stringify(got));
          });
        },
      );
      r.on("error", (e: any) => console.log(name, "client error", e.code));
      r.on("close", () => {
        setTimeout(() => {
          console.log(name, "server events", events.join(","));
          done();
        }, 50);
      });
      r.end("{}");
    });
  });
}

async function main() {
  await run("empty-204", 204, "");
  await run("body-200", 200, "bye");
}
main();
