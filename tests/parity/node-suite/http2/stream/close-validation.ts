import * as http2 from "node:http2";

const server = http2.createServer();
let client: any;
try {
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  client = http2.connect(`http://127.0.0.1:${(server.address() as any).port}`);
  await new Promise<void>((resolve, reject) => {
    client.on("error", reject);
    client.on("connect", resolve);
  });
  const request = client.request();
  for (const value of ["string", 1.01, -1, 2 ** 32]) {
    try {
      request.close(value as any);
      console.log(String(value), "accepted");
    } catch (error: any) {
      console.log(String(value), error.name, error.code);
    }
  }
  request.destroy();
} finally {
  client?.destroy();
  await new Promise<void>((resolve) => server.close(() => resolve()));
}
