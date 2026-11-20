import { createServer } from "node:http2";

const server = createServer();
try {
  await new Promise<void>((resolve, reject) => {
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address() as any;
      console.log(typeof address.port, address.address, address.family);
      resolve();
    });
  });
} finally {
  await new Promise<void>((resolve) => server.close(() => resolve()));
}
