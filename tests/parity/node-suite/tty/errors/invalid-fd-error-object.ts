// What a program can observe of the error a tty stream throws for a bad fd:
// a real RangeError with code ERR_INVALID_FD.
import * as tty from "node:tty";

for (const fd of [-1, 1.5]) {
  try {
    new tty.ReadStream(fd);
    console.log(fd, "no throw");
  } catch (e: any) {
    console.log(fd, "name:", e.name, "code:", e.code);
    console.log(fd, "message:", e.message);
    console.log(fd, "instanceof Error/RangeError:", e instanceof Error, e instanceof RangeError);
    console.log(fd, "keys:", JSON.stringify(Object.keys(e)));
    console.log(fd, "String:", String(e));
    console.log(fd, "stack is a string:", typeof e.stack === "string");
    console.log(fd, "tag:", Object.prototype.toString.call(e));
  }
}
