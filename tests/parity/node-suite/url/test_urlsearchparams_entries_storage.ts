// URLSearchParams keeps its entries in an internal array of [name, value]
// pairs. That array and its pairs are heap references held in value slots of
// the params object, so they must survive collections that move them and must
// never be visible as own state. Every method is exercised between rounds of
// allocation, and the objects that hold a URLSearchParams are stringified.

function churn(n: number): number {
  let keep: any[] = [];
  let total = 0;
  for (let i = 0; i < n; i++) {
    const o = { i, s: "v" + i, a: [i, i + 1, i + 2] };
    if (i % 97 === 0) keep.push(o);
    if (keep.length > 64) keep = keep.slice(32);
    total += o.a.length;
  }
  return total + keep.length;
}

function dump(label: string, p: URLSearchParams): void {
  const viaForEach: string[] = [];
  p.forEach((value, name, self) => viaForEach.push(name + "=" + value + (self === p ? "" : "!")));
  console.log(label, "str", p.toString(), "size", p.size);
  console.log(label, "entries", JSON.stringify([...p.entries()]));
  console.log(label, "keys", JSON.stringify([...p.keys()]), "values", JSON.stringify([...p.values()]));
  console.log(label, "spread", JSON.stringify([...p]), "forEach", viaForEach.join("&"));
  console.log(label, "json", JSON.stringify(p), JSON.stringify({ p }), JSON.stringify([p]));
  console.log(label, "own", JSON.stringify(Object.keys(p)), JSON.stringify(Object.entries(p)));
}

const p = new URLSearchParams("a=1&b=2&a=3");
dump("init", p);
churn(40000);
p.append("c", "4");
p.append("a", "5");
dump("append", p);
console.log("get", p.get("a"), p.get("b"), p.get("zz"), JSON.stringify(p.getAll("a")), JSON.stringify(p.getAll("zz")));
console.log("has", p.has("a"), p.has("a", "3"), p.has("a", "9"), p.has("zz"));
churn(40000);
p.set("a", "7");
dump("set", p);
p.delete("b");
dump("delete", p);
p.append("b", "x");
p.append("b", "y");
p.delete("b", "x");
dump("delete2", p);
churn(40000);
p.sort();
dump("sort", p);

// Many entries: the entries array itself is reallocated and promoted.
const big = new URLSearchParams();
for (let i = 0; i < 300; i++) {
  big.append("k" + (i % 17), "v" + i);
  if (i % 50 === 0) churn(20000);
}
console.log("big", big.size, big.get("k3"), big.getAll("k16").length, big.toString().length);
big.sort();
console.log("big sorted", [...big.keys()].slice(0, 5).join(","), [...big.values()].slice(-3).join(","));
for (let i = 0; i < 17; i += 2) big.delete("k" + i);
churn(40000);
console.log("big deleted", big.size, big.has("k0"), big.has("k1"), big.toString().slice(0, 40));

// The init shapes.
const fromRecord = new URLSearchParams({ x: "1", y: "2" });
const fromPairs = new URLSearchParams([["m", "1"], ["n", "2"], ["m", "3"]]);
const fromOther = new URLSearchParams(fromPairs);
const fromIter = new URLSearchParams(new Map([["q", "1"], ["r", "2"]]));
churn(40000);
fromOther.append("o", "9");
dump("record", fromRecord);
dump("pairs", fromPairs);
dump("other", fromOther);
dump("iter", fromIter);

// A URL's searchParams writes through to the URL and back.
const url = new URL("https://example.com/p?x=1&y=2");
const sp = url.searchParams;
sp.append("z", "3");
churn(40000);
sp.set("x", "8");
console.log("url", url.href, url.search, sp === url.searchParams);
url.search = "?only=1";
churn(40000);
dump("url.search=", url.searchParams);
console.log("url json", JSON.stringify({ url, sp: url.searchParams }));

// Holders: objects, arrays, Maps and closures that keep a URLSearchParams.
const holder = { name: "h", params: new URLSearchParams("h=1"), list: [new URLSearchParams("l=1"), new URLSearchParams("l=2")] };
const byName = new Map<string, URLSearchParams>([["m", new URLSearchParams("mm=1")]]);
const read = ((q: URLSearchParams) => () => q.get("c"))(new URLSearchParams("c=closure"));
churn(60000);
holder.params.append("h", "2");
holder.list[1].append("l", "3");
byName.get("m")!.set("mm", "2");
console.log("holder", JSON.stringify(holder), holder.params.toString(), holder.list.map(String).join("|"));
console.log("map", byName.get("m")!.toString(), "closure", read());

// A subclass keeps the native entries behind the instance.
class Query extends URLSearchParams {
  tag = "q";
  first(name: string): string | null {
    return this.get(name);
  }
}
const q = new Query("s=1&t=2");
q.append("s", "3");
churn(40000);
console.log("subclass", q.first("s"), JSON.stringify(q.getAll("s")), q.toString(), q.size, q.tag, JSON.stringify(Object.keys(q)));
