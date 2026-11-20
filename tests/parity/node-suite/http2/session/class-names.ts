import * as http2 from "node:http2";

const server = http2.createServer();
let client: any;
try {
  const serverName = new Promise<string>((resolve) => {
    server.on("session", (session: any) => resolve(session.constructor.name));
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  client = http2.connect(`http://127.0.0.1:${(server.address() as any).port}`);
  await new Promise<void>((resolve, reject) => {
    client.on("error", reject);
    client.on("connect", resolve);
  });
  console.log("client:", client.constructor.name);
  console.log("server:", await serverName);
} finally {
  client?.destroy();
  await new Promise<void>((resolve) => server.close(() => resolve()));
}
