import { Readable } from "node:stream";
// A user subclass of Readable implementing _read() emits via push().
class Once extends Readable {
  private done = false;
  _read() {
    if (!this.done) {
      this.push("once");
      this.push(null);
      this.done = true;
    }
  }
}
const r = new Once();
const out: string[] = [];
r.on("data", (c) => out.push(String(c)));
r.on("end", () => console.log("joined:", out.join("")));
