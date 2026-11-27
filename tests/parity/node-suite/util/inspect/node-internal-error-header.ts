// Node's internal errors name their code in the stack and in toString();
// a user error that merely carries a code, and a system error, do not.
import * as fs from "node:fs";
import * as util from "node:util";

function show(label: string, error: any): void {
  console.log(label, String(error));
  console.log(label, String(error.stack).split("\n")[0]);
  console.log(label, util.inspect(error).split("\n")[0]);
}

try {
  Buffer.alloc(-1);
} catch (error) {
  show("internal", error);
}
const user: any = new TypeError("bad thing");
user.code = "ERR_USER";
show("user", user);
try {
  fs.readFileSync("/perry-parity-definitely-missing");
} catch (error) {
  show("system", error);
}
