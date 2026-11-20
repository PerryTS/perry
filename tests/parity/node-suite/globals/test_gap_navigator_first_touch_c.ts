// First-touch order (c): perry births Navigator's family on its first
// observation; the keys, descriptors and identities printed after the
// touches are the same whichever observation came first (the
// test_gap_navigator_first_touch_{a,b,c} programs differ only in order).
const G: any = globalThis as any;
const touch: string[] = [];
touch.push("instanceof:" + String(({}) instanceof G.Navigator));
touch.push("prototype:" + typeof G.Navigator.prototype);
touch.push("navigator:" + typeof G.navigator);
G.Navigator.x = 1;
Object.defineProperty(G.Navigator, "name", { value: "Renamed" });
touch.push("write:" + String(G.Navigator.x));
touch.sort();
const N: any = G.Navigator;
const P: any = N.prototype;
const out: string[] = [touch.join(" ")];
out.push("name: " + N.name);
out.push("ctor keys: " + JSON.stringify(Reflect.ownKeys(N).map(String)));
out.push("proto keys: " + JSON.stringify(Reflect.ownKeys(P).map(String)));
const pd = Object.getOwnPropertyDescriptor(N, "prototype")!;
out.push("prototype desc: " + [pd.value === P, pd.writable, pd.enumerable, pd.configurable].join(","));
for (const k of Reflect.ownKeys(P)) {
  const d: any = Object.getOwnPropertyDescriptor(P, k)!;
  out.push(String(k) + ": " + ("value" in d ? "data" : "get " + d.get.name) + "," + String(d.enumerable) + "," + String(d.configurable));
}
out.push("identities: " + [G.navigator instanceof N, Object.getPrototypeOf(G.navigator) === P, P.constructor === N, G.navigator === G.navigator].join(","));
console.log(out.join("\n"));
