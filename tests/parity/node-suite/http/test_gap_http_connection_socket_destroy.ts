import { createServer as createHttpServer } from "node:http";
import { createServer as createHttp2Server } from "node:http2";
import { connect } from "node:net";

async function check(name: string, createServer: any): Promise<void> {
  let accepted = 0;
  let socketClosed = 0;
  let requests = 0;
  let clientClosed = false;
  let serverClosed = false;
  const timeout = setTimeout(() => {
    console.log(name, "timeout");
    process.exit(1);
  }, 2000);

  const server = createServer((_req: any, res: any) => {
    requests++;
    res.end();
  });
  server.on("connection", (socket: any) => {
    accepted++;
    socket.on("close", () => socketClosed++);
    socket.destroy();
    socket.destroy();
  });

  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const client = connect((server.address() as any).port, "127.0.0.1");
  client.on("error", () => {});
  await new Promise<void>((resolve) => client.on("close", resolve));
  clientClosed = true;
  await new Promise<void>((resolve) => server.close(resolve));
  serverClosed = true;
  clearTimeout(timeout);
  console.log(name, "accepted", accepted);
  console.log(name, "socket close", socketClosed);
  console.log(name, "client close", clientClosed);
  console.log(name, "server close", serverClosed);
  console.log(name, "requests", requests);
}

await check("http", createHttpServer);
await check("http2", createHttp2Server);
