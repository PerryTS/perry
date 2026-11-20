// #12540: a native handle (HANDLE_TAG) is a JS object for reflection too:
// Object.keys, expando property get/set, instanceof, `in`, spread, Map/Set
// identity keys, Object.is and Array.includes. Byte-identical to
// node --experimental-strip-types. (JSON.stringify of a fetch handle is
// `null` by the #2154 design, both before and after #12529; not covered.)

function show(name: string, h: any, ctor: any): void {
  console.log(name, "keys", Object.keys(h).length, "| instanceof", h instanceof ctor, "| instanceof Object", h instanceof Object);
  h.expando = 41;
  h.expando = h.expando + 1;
  console.log(name, "expando", h.expando, "| in", "expando" in h, "| missing", h.zzMissing);
  const key = "dyn" + name.length;
  h[key] = "d";
  h[7] = "seven";
  console.log(name, "computed", h[key], h["dyn" + name.length], h[7], "| in", key in h, 7 in h);
  console.log(name, "values", JSON.stringify(Object.values(h).filter((v) => v !== "seven")), "| entries", Object.entries(h).length);
  console.log(name, "keys after", JSON.stringify(Object.keys(h).filter((k) => k !== "7")), "| delete", delete h.expando, "expando" in h);
  console.log(name, "tag", Object.prototype.toString.call(h), "| isArray", Array.isArray(h), "| typeof fn", typeof (() => h));
  const m = new Map<any, string>();
  m.set(h, name);
  m.set({}, "other");
  const s = new Set<any>([h, h]);
  console.log(name, "map", m.get(h), m.has(h), m.size, "| set", s.size, s.has(h));
  console.log(name, "Object.is", Object.is(h, h), "| includes", [1, h].includes(h), "| indexOf", [h].indexOf(h));
  const arr = [...[h]];
  console.log(name, "spread", arr.length, arr[0] === h, "| objspread", typeof { ...{ v: h } }.v);
  console.log(name, "Object()", Object(h) === h, "| proto", Object.getPrototypeOf(h) === ctor.prototype);
}

show("Headers", new Headers(), Headers);
show("Blob", new Blob(["xy"]), Blob);
show("Response", new Response("r"), Response);
show("URL", new URL("http://example.com/"), URL);
show("AbortController", new AbortController(), AbortController);
// Negative control: a plain class instance.
class Plain { a = 1; }
show("Plain", new Plain(), Plain);

// An iterable handle spreads and destructures like any iterable object.
const hd: any = new Headers({ a: "1", b: "2" });
console.log(JSON.stringify([...hd]), JSON.stringify(Array.from(hd.keys())));
for (const [k, v] of hd) console.log("entry", k, v);

// Awaiting a handle resolves to the handle itself (it is not a thenable).
(async () => {
  const r: any = await hd;
  console.log("await", r === hd, typeof r);
})();
