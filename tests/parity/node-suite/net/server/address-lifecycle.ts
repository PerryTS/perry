import { createServer } from "node:net";

const server = createServer();
console.log("before:", server.listening, server.address());

try {
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const address = server.address() as any;
  console.log(
    "during:",
    server.listening,
    address.address,
    address.family,
    typeof address.port,
    address.port > 0,
  );
} finally {
  if (server.listening) {
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
  server.removeAllListeners();
}

console.log("after:", server.listening, server.address());
