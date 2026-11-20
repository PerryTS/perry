// URLSearchParams instances have no own properties in node: the entries and
// the owning URL are internal state. Every own-key reader must agree, and the
// behaviour backed by that state must keep working.

function ownKeys(label: string, o: any): void {
  // for-in also walks URLSearchParams.prototype; its own part is what an
  // instance exposes.
  const forIn: string[] = [];
  for (const k in o) if (Object.prototype.hasOwnProperty.call(o, k)) forIn.push(k);
  console.log(label, "keys", JSON.stringify(Object.keys(o)));
  console.log(label, "for-in", JSON.stringify(forIn));
  console.log(label, "names", JSON.stringify(Object.getOwnPropertyNames(o)));
  console.log(label, "ownKeys", JSON.stringify(Reflect.ownKeys(o).map(String)));
  console.log(label, "json", JSON.stringify(o));
  console.log(label, "descs", JSON.stringify(Object.keys(Object.getOwnPropertyDescriptors(o))));
  console.log(label, "entries", JSON.stringify(Object.entries(o)));
  console.log(label, "spread", JSON.stringify(Object.keys({ ...o })));
  console.log(label, "has _entries", Object.prototype.hasOwnProperty.call(o, "_entries"), "_entries" in o, o._entries === undefined);
  console.log(label, "has _owner", Object.prototype.hasOwnProperty.call(o, "_owner"), "_owner" in o, o._owner === undefined);
}

const empty = new URLSearchParams();
ownKeys("empty", empty);

const p = new URLSearchParams("a=1&b=2");
p.append("c", "3");
p.set("a", "9");
p.delete("b");
ownKeys("used", p);
console.log("used get", p.get("a"), p.get("c"), p.has("b"), p.size, p.toString());
console.log("used spread", JSON.stringify([...p]));

const copy = new URLSearchParams(p);
ownKeys("copy", copy);
console.log("copy", copy.toString());

const url = new URL("https://example.com/path?x=1&y=2");
const sp = url.searchParams;
ownKeys("url.searchParams", sp);
sp.append("z", "3");
sp.set("x", "7");
console.log("url sync", url.search, url.href);
ownKeys("url.searchParams after use", url.searchParams);
console.log("same object", url.searchParams === sp);

// An expando is an ordinary own property, listed after nothing.
const ex: any = new URLSearchParams("q=1");
ex.extra = 5;
ownKeys("expando", ex);
console.log("expando still works", ex.get("q"), ex.toString());

// A plain object named like the old slot is just a record.
const rec = new URLSearchParams({ _entries: "x", _owner: "y" } as any);
console.log("record", rec.toString(), JSON.stringify([...rec]));
const rec2 = new URLSearchParams({ _entries: [["a", "b"]] } as any);
console.log("record2", rec2.toString());
