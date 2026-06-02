// #3851: Server[Symbol.asyncDispose] should be a safe function-valued
// property. On an unbound server Node returns a rejected Promise with
// ERR_SERVER_NOT_RUNNING.
import { createServer } from "node:http";

const server = createServer();
const dispose = (server as any)[Symbol.asyncDispose];

console.log("asyncDispose type:", typeof dispose);

const result = dispose.call(server);
console.log("asyncDispose promise:", typeof result?.then, Object.prototype.toString.call(result));

try {
  await result;
  console.log("asyncDispose result: fulfilled");
} catch (e: any) {
  console.log("asyncDispose result:", e.name, e.code, e.message);
}
