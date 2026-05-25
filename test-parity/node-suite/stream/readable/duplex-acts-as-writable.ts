import { Duplex } from "node:stream";
// A Duplex stream has both read and write sides.
const received: string[] = [];
const d = new Duplex({
  read() { this.push(null); }, // empty readable
  write(c, _e, cb) { received.push(String(c)); cb(); },
});
d.on("data", () => {});
d.write("x");
d.end("y");
d.on("finish", () => console.log("received:", received.join(",")));
