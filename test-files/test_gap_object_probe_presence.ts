// Object model step 5: the probe answers presence from a record's complete
// key list whatever its generation (Object.prototype, mutated class
// prototypes), indexes keys written before their program atom existed by
// text, walks plain unmarked objects, and hands accessors back to the slow
// path. Every read below goes through an unproven receiver.

function read(o: any, k: string): any {
  switch (k) {
    case "alpha": return o.alpha;
    case "beta": return o.beta;
    case "gamma": return o.gamma;
    case "missing": return o.missing;
    case "toString": return typeof o.toString;
    case "hasOwnProperty": return typeof o.hasOwnProperty;
    case "valueOf": return typeof o.valueOf;
    case "constructor": return typeof o.constructor;
    case "__proto__": return o.__proto__ === null ? "null" : typeof o.__proto__;
    case "max": return typeof o.max;
    case "parse": return typeof o.parse;
    case "log": return typeof o.log;
    case "path": return o.path;
    case "name": return o.name;
    case "late": return o.late;
    default: return o[k];
  }
}

const keys = ["alpha", "beta", "gamma", "missing", "toString", "hasOwnProperty",
  "valueOf", "constructor", "__proto__", "max", "parse", "log", "path", "name", "late"];

function dump(label: string, o: any) {
  const out: string[] = [];
  for (const k of keys) {
    let v: any;
    try { v = read(o, k); } catch (e) { v = "throw"; }
    out.push(k + "=" + String(v));
  }
  console.log(label + ": " + out.join(" "));
}

// 1. plain literal, then a mutated Object.prototype (generation != 0).
dump("plain", { alpha: 1 });
(Object.prototype as any).gamma = "fromObjProto";
dump("plain+objproto", { alpha: 2 });
Object.defineProperty(Object.prototype, "beta", { value: "hidden", enumerable: false, configurable: true, writable: true });
dump("plain+objproto2", { alpha: 3 });
delete (Object.prototype as any).gamma;
delete (Object.prototype as any).beta;
dump("plain-after-delete", { alpha: 4 });

// 2. class prototype mutated after definition.
class Base {
  alpha = 10;
  get path(): string { return "path:" + this.alpha; }
  hello() { return "hi"; }
}
class Derived extends Base { beta = 20; }
dump("derived", new Derived());
(Base.prototype as any).name = "baseName";
Object.defineProperty(Base.prototype, "late", { get() { return "lateGetter"; }, configurable: true });
dump("derived-mutated", new Derived());
const d = new Derived();
(d as any).name = "own";
dump("derived-own", d);

// 3. keys from data (no program atom at write time), read by name and computed.
const big: any = {};
for (let i = 0; i < 40; i++) big["k_dyn_" + i] = i;
big["late" + ""] = "dataLate";
big[["na", "me"].join("")] = "dataName";
dump("big", big);
const parsed = JSON.parse('{"alpha":1,"zeta_only_in_data":2,"name":"json"}');
dump("json", parsed);
console.log("computed", big["k_dyn_" + 7], big["k_dyn_" + 99], parsed["zeta_only_in_data"], parsed["zeta" + "_only_in_data"]);

// 4. plain unmarked chains: Object.create(null) dictionaries and prototypes.
const dict: any = Object.create(null);
dict.alpha = "d";
dump("dict", dict);
const protoObj: any = Object.create(null);
protoObj.beta = "pb";
const child: any = Object.create(protoObj);
child.alpha = "ca";
dump("child-of-dict", child);
const mid: any = Object.create({ gamma: "g" });
dump("mid", Object.create(mid));
Object.setPrototypeOf(protoObj, { name: "relinked" });
dump("child-relinked", child);

// 5. builtin namespaces and prototypes keep their methods.
dump("Math", Math);
dump("JSON", JSON);
dump("console", console);
dump("array", [1, 2, 3]);
dump("date", new Date(0));
dump("map", new Map());
dump("regex", /x/);
dump("err", new Error("e"));

// 6. accessors on prototypes and own accessors.
const withGetter: any = Object.create({ get path() { return "protoPath:" + this.alpha; } });
withGetter.alpha = "A";
dump("proto-getter", withGetter);
const ownGetter: any = { alpha: 1, get path() { return "ownPath"; } };
dump("own-getter", ownGetter);
const setterOnly: any = Object.create({ set path(v: any) {} });
dump("setter-only", setterOnly);
let reps = "";
for (let i = 0; i < 5; i++) reps += read(new Derived(), "path") + read(withGetter, "path");
console.log(reps);
