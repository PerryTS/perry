// #3527: native `node:http` constructor classes expose a real `.prototype`
// object so userland subclassing — `Object.create(http.IncomingMessage.
// prototype)` (Express), `util.inherits(...)` — works instead of throwing
// "Object prototype may only be an Object or null".
import http from "node:http";
import https from "node:https";
import util from "node:util";

for (const name of [
  "IncomingMessage",
  "ServerResponse",
  "OutgoingMessage",
  "ClientRequest",
  "Server",
  "Agent",
] as const) {
  console.log(`http.${name}.prototype:`, typeof (http as any)[name].prototype);
}
console.log("https.Server.prototype:", typeof https.Server.prototype);
console.log("https.Agent.prototype:", typeof https.Agent.prototype);

// Express's request/response prototype pattern.
const req = Object.create(http.IncomingMessage.prototype);
const res = Object.create(http.ServerResponse.prototype);
console.log("Object.create(IncomingMessage.prototype) is object:", typeof req === "object");
console.log("Object.create(ServerResponse.prototype) is object:", typeof res === "object");

// util.inherits over a native http class.
function MyMessage() {}
util.inherits(MyMessage, http.IncomingMessage);
console.log(
  "util.inherits links prototype chain:",
  Object.getPrototypeOf(MyMessage.prototype) === http.IncomingMessage.prototype,
);
