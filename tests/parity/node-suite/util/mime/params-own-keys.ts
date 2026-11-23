// MIMEParams instances have no own properties in node: the parameter
// entries are internal state. Every own-key reader must agree, and the
// behaviour backed by that state must keep working.
import { MIMEParams, MIMEType } from "node:util";

function ownKeys(label: string, o: any): void {
  const forIn: string[] = [];
  for (const k in o) if (Object.prototype.hasOwnProperty.call(o, k)) forIn.push(k);
  console.log(label, "keys", JSON.stringify(Object.keys(o)));
  console.log(label, "for-in", JSON.stringify(forIn));
  console.log(label, "names", JSON.stringify(Object.getOwnPropertyNames(o)));
  console.log(label, "ownKeys", JSON.stringify(Reflect.ownKeys(o).map(String)));
  console.log(label, "descs", JSON.stringify(Object.keys(Object.getOwnPropertyDescriptors(o))));
  console.log(label, "spread", JSON.stringify(Object.keys({ ...o })));
  console.log(label, "has _entries", Object.prototype.hasOwnProperty.call(o, "_entries"), "_entries" in o, o._entries === undefined);
}

const empty = new MIMEParams();
ownKeys("empty", empty);

const mime = new MIMEType("text/plain; charset=utf-8; x=1");
const p = mime.params;
ownKeys("parsed", p);
p.set("y", "2");
p.delete("x");
ownKeys("used", p);
console.log("used", p.get("charset"), p.has("x"), p.get("y"), p.toString(), String(mime));
console.log("entries", JSON.stringify([...p.entries()]));

const ex: any = new MIMEParams();
ex.extra = 5;
ex.set("q", "1");
ownKeys("expando", ex);
console.log("expando still works", ex.get("q"), ex.toString());
