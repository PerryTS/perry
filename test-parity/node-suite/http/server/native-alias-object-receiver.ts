// `Server.call(receiver)` on a receiver born by `Object()` (a runtime-born
// record that does not admit plain stores): after the call, a property the
// receiver lacks reads from the server. Perry used to answer `undefined` for
// the second read.
//
// Node prints:
//   undefined
//   0
import http from "node:http";

const receiver: any = Object();
Object.setPrototypeOf(receiver, http.Server.prototype);
function readTimeout(value: any) {
  return value.timeout;
}
console.log(readTimeout(receiver));
http.Server.call(receiver);
console.log(readTimeout(receiver));
