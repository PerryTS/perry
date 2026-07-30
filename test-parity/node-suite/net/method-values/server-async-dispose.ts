import * as net from "node:net";

const server = new net.Server();

console.log("asyncDispose typeof:", typeof server[Symbol.asyncDispose]);
console.log("listening before:", (server as any)["listening"]);

const result = server[Symbol.asyncDispose]();
console.log("asyncDispose result then:", typeof result?.then);

try {
  await result;
  console.log(
    "asyncDispose resolved:",
    (server as any)["listening"],
    (server as any).closed,
  );
} catch (error: any) {
  console.log("asyncDispose rejected:", error.name, error.code);
}
