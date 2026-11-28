// A system error from a bad descriptor carries errno, code and syscall as
// ordinary own properties, in Node's order.
import * as fs from "node:fs";

try {
  fs.readSync(987654, Buffer.alloc(1));
} catch (error: any) {
  console.log(Object.keys(error).join(","), JSON.stringify(error));
}
try {
  fs.fstatSync(987654);
} catch (error: any) {
  console.log(Object.keys(error).join(","), error.code, error.syscall);
}
