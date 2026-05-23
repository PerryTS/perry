import { Duplex } from "node:stream";
// Custom Duplex subclass with _read + _write.
class Echo extends Duplex {
  _read() {}
  _write(c: any, _e: any, cb: any) { this.push(c); cb(); }
}
const d = new Echo();
const out: string[] = [];
d.on("data", (c) => out.push(String(c)));
d.write("ping");
d.end(() => console.log("got:", out.join("")));
