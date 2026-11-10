import { createServer } from "node:http";

const server = createServer();
console.log(
  "close self:",
  server.close((error: any) => {
    console.log("callback:", error.name, error.code);
  }) === server,
);

try {
  await server[Symbol.asyncDispose]();
} catch (error: any) {
  console.log("dispose:", error.name, error.code);
}
