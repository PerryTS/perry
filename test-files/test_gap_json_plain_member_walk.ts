// #10696: an object-valued member that no `toJSON` can reach is walked
// directly, and one `Object.prototype` verdict serves every such member until
// something runs user code. Every route that runs user code mid-walk must
// still make later members see a `toJSON` installed by it, and every exotic
// member kind must keep its own serialization.
const log = (label: string, x: any) => console.log(label, JSON.stringify(x));
const OP = Object.prototype as any;
const install = () => {
  OP.toJSON = function () { return "OP"; };
};

// Nested plain records: literals, parsed trees, wide and 1-field levels.
log("nest", { a: { b: { c: { d: { e: 1 } } } } });
log("parsed", JSON.parse('{"a":{"b":{"c":1}},"d":{"e":[1,{"f":2}]}}'));
log("wide", { a: { p: 1, q: "two", r: true, s: null, t: 5.5, u: { v: 1 } }, b: 2 });
log("siblings", { a: { x: 1 }, b: { y: 2 }, c: { z: { w: 3 } } });
log("empty", { a: {}, b: { c: {} } });
log("undef", { a: { b: undefined, c: 1 }, d: { e: undefined } });
log("fn", { a: { b() { return 1; }, c: 2 }, d: { e: Symbol("s"), f: 3 } });
log("index keys", { a: { b: 1, 2: "two", 1: "one" } });
log("escaped keys", { a: { 'q"uote': 1, "back\\slash": 2, "nl\n": 3, "é": 4, "😀": 5 } });
const holey: any = { p: 1, q: 2, r: 3 };
delete holey.q;
log("holes", { a: holey, b: { c: holey } });
log("internal-looking keys", { a: { __perry_collection_backing__: 1, b: 2 } });
const shared = { s: 1 };
log("shared", { a: shared, b: shared, c: [shared, { d: shared }] });
log("root wide", { a: 1, b: 2, c: 3, d: 4, e: 5, f: 6, g: { h: 7 } });

// toJSON on a member, on a grandchild, returning a plain tree, and its key.
log("member toJSON", { a: { toJSON(k: string) { return "k=" + k; } }, b: { c: 1 } });
log("grandchild toJSON", { a: { b: { toJSON() { return [1, { c: 2 }]; } } }, d: { e: 3 } });
log("toJSON result tree", { a: { toJSON() { return { b: { c: { toJSON() { return "inner"; } } } }; } } });
log("toJSON undefined", { a: { toJSON() { return undefined; } }, b: { c: 1 } });

// Object.prototype.toJSON installed mid-walk, from each kind of callback.
log("by member toJSON", { a: { toJSON() { install(); return 1; } }, b: { c: 1 }, d: { e: { f: 1 } } });
delete OP.toJSON;
log("by grandchild toJSON", { a: { b: { toJSON() { install(); return 1; } }, c: { d: 1 } }, e: { f: 1 } });
delete OP.toJSON;
log("by getter", { get a() { install(); return 1; }, b: { c: 1 } });
delete OP.toJSON;
log("by nested getter", { a: { get b() { install(); return 1; } }, c: { d: 1 } });
delete OP.toJSON;
log("by array element", { a: [{ toJSON() { install(); return 1; } }], b: { c: 1 } });
delete OP.toJSON;
(BigInt.prototype as any).toJSON = function () { install(); return "big"; };
log("by BigInt toJSON", { a: { n: 1n }, b: { c: 1 } });
delete (BigInt.prototype as any).toJSON;
delete OP.toJSON;
log("by nested stringify", { a: { toJSON() { return JSON.stringify({ x: { y: 1 } }); } }, b: { c: 1 } });

// Object.prototype.toJSON present for a whole call, then removed.
install();
log("installed", { a: { b: 1 } });
log("installed parsed", JSON.parse('{"a":{"b":1}}'));
delete OP.toJSON;
log("removed", { a: { b: 1 } });

// Members that inherit a toJSON, or carry one as an accessor.
const proto = { toJSON() { return "P"; } };
const viaSet: any = { v: 1 };
Object.setPrototypeOf(viaSet, proto);
const viaCreate = Object.create(proto);
viaCreate.v = 2;
class WithToJSON { v = 3; toJSON() { return "C"; } }
class Plain { v = 4; w = { x: 5 }; }
log("inherited", { a: viaSet, b: viaCreate, c: new WithToJSON(), d: new Plain() });
log("accessor toJSON", { a: { get toJSON() { return () => "acc"; } }, b: { c: 1 } });
log("null proto", { a: Object.assign(Object.create(null), { b: 1 }) });

// Exotic members keep their own serialization.
class MyArray extends Array<number> {}
const sub = new MyArray();
sub.push(1, 2);
const nonEnum: any = { visible: 1 };
Object.defineProperty(nonEnum, "hidden", { value: 2, enumerable: false });
log("exotic", {
  date: new Date(0),
  map: new Map([[1, 2]]),
  set: new Set([1]),
  re: /x/g,
  num: new Number(3),
  str: new String("s"),
  bool: new Boolean(false),
  u8: new Uint8Array([1, 2]),
  sub,
  err: Object.assign(new Error("e"), { code: 7 }),
  url: new URL("https://example.com/p?q=1"),
  nonEnum,
  nested: { date: new Date(0), deeper: { num: new Number(4) } },
});

// Deep plain chains, and a cycle through them past the fast depth.
const head: any = {};
let cur = head;
for (let i = 0; i < 200; i++) {
  cur.next = { i };
  cur = cur.next;
}
console.log("deep ok", JSON.stringify(head).length);
cur.next = head;
try {
  JSON.stringify(head);
  console.log("cycle: no throw");
} catch (e) {
  console.log("cycle:", (e as Error).constructor.name);
}
