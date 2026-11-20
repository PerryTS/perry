import { createServer } from "node:net";

const server = createServer();

async function listen(...args: any[]) {
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    (server.listen as any)(...args, function (this: any) {
      console.log(
        "callback:",
        this === server,
        server.listening,
        (server.address() as any).family,
      );
      resolve();
    });
  });
  await new Promise<void>((resolve) => server.close(() => resolve()));
}

try {
  await listen(0, "127.0.0.1");
  await listen({ port: 0, host: "127.0.0.1" });
  await listen(0, "127.0.0.1", 8);
} finally {
  if (server.listening) {
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
  server.removeAllListeners();
}
