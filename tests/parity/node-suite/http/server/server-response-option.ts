import http from "node:http";

class MyResponse extends http.ServerResponse {
  tag = "field";
  fromConstructor: string | undefined;
  constructor(req: any, options?: any) {
    super(req, options);
    this.fromConstructor = req.method;
  }
  override writeHead(...args: any[]): this {
    this.setHeader("x-from-subclass", "1");
    return (super.writeHead as any)(...args);
  }
  send(body: string) {
    this.writeHead(201);
    this.end(body);
  }
}

const server = http.createServer({ ServerResponse: MyResponse }, (req, res) => {
  const mine = res as unknown as MyResponse;
  console.log(res instanceof MyResponse, res instanceof http.ServerResponse, mine.tag, mine.fromConstructor);
  console.log(res.req === req, res.socket === req.socket, res.headersSent);
  res.on("finish", () => console.log("finish", req.url));
  if (req.url === "/send") return mine.send("sent");
  if (req.url === "/later") {
    setTimeout(() => {
      res.writeHead(200);
      res.write("a");
      res.end("b");
    }, 5);
    return;
  }
  res.writeHead(200);
  res.end("ok");
});
console.log("server instanceof http.Server:", server instanceof http.Server);
console.log({} instanceof http.Server);

const agent = new http.Agent({ keepAlive: true, maxSockets: 1 });
const get = (port: number, path: string) =>
  new Promise<void>((resolve) => {
    http.get({ port, host: "127.0.0.1", path, agent }, (res) => {
      let body = "";
      res.setEncoding("utf8");
      res.on("data", (chunk) => (body += chunk));
      res.on("end", () => {
        console.log(path, res.statusCode, res.headers["x-from-subclass"], "date" in res.headers, res.headers.connection, body);
        resolve();
      });
    });
  });

server.listen(0, "127.0.0.1", async () => {
  const { port } = server.address() as { port: number };
  for (const path of ["/", "/send", "/later"]) await get(port, path);
  agent.destroy();
  server.close(() => console.log("closed"));
});
