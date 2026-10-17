// Gap test for #12697: a URLSearchParams is born on a declared runtime
// layout, an ordinary shape, so a type-erased read of `size` or a method used
// to skip the by-kind URLSearchParams probe and answer `undefined`
// (`p.size, typeof p.append` printed `undefined undefined`; node prints
// `0 function`). Both now come from %URLSearchParams.prototype% through the
// layout's prototype family. Byte-identical to node.
const p = new URLSearchParams();
console.log(p.size, typeof p.append);
const anyP: any = new URLSearchParams("a=1&b=2&a=3");
console.log(anyP.size, typeof anyP.get, typeof anyP.forEach);
console.log(anyP.get("a"), anyP.getAll("a").join(","), anyP.has("b"), anyP.has("z"));
anyP.append("c", "4");
anyP.set("b", "9");
anyP.delete("a");
console.log(anyP.size, anyP.toString());
const proto = Object.getPrototypeOf(anyP);
console.log(proto === URLSearchParams.prototype, anyP.append === URLSearchParams.prototype.append);
const desc = Object.getOwnPropertyDescriptor(URLSearchParams.prototype, "size")!;
console.log(typeof desc.get, desc.set, desc.configurable);
console.log(desc.get!.call(anyP));
try {
  desc.get!.call({});
  console.log("no throw");
} catch (e) {
  console.log((e as Error).name);
}
const has = URLSearchParams.prototype.has;
console.log(has.call(anyP, "c"), has.call(anyP, "a"));
console.log("size" in anyP);
const u = new URL("http://x/?q=1&r=2");
const sp: any = u.searchParams;
console.log(sp.size, sp.get("r"));
class Sub extends URLSearchParams {}
const s: any = new Sub("k=v&k2=v2");
console.log(s.size, s.get("k2"), typeof s.append);
const items: any[] = [new URLSearchParams("x=1"), { size: 5 }];
for (const it of items) console.log(it.size);
