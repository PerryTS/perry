// The pre-class inherits pattern over http.Server:
// `http.Server.call(this, …)` plus
// `Object.setPrototypeOf(F.prototype, http.Server.prototype)`. After
// `listen`, `net.Server.prototype` members (`address`, `close`, `listening`)
// reach the instance through the prototype chain. The
// instance aliases the native server; each member must run on that server,
// not refuse the instance with "TypeError: Illegal receiver".
import * as http from "node:http";

const H: any = http;

function TestServer(this: any) {
  H.Server.call(this, () => {});
}
Object.setPrototypeOf(TestServer.prototype, H.Server.prototype);

const server: any = new (TestServer as any)();
console.log("listening before", server.listening);
const ret = server.listen(0, "127.0.0.1", () => {
  console.log("listen returns this", ret === server);
  console.log("listening after", server.listening);
  const a = server.address();
  console.log("address", typeof a, a && a.address, a && typeof a.port, a && a.port > 0);
  // The member read as a value and called on the instance.
  const viaProto = H.Server.prototype.address.call(server);
  console.log("address via prototype value", viaProto && viaProto.port === a.port);
  const c = server.close(() => console.log("close callback"));
  console.log("close returns this", c === server);
});
