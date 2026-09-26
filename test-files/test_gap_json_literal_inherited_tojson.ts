// #10529 / #10696: object literals are anonymous shape classes, so the plain
// record emitters now serve them. Every way a literal (or a JSON.parse record)
// can still INHERIT a `toJSON` must keep working through those emitters.
const log = (x: any) => console.log(JSON.stringify(x));

// Plain literals through the flat / record / template / nested paths.
log({ hello: "world" });
log({ id: 1, name: "Ada", email: "ada@example.com", active: true, tags: ["a", "b"] });
log({ a: { b: { c: { d: { e: 1 } } } } });
log([{ a: 1 }, { a: 2 }, { a: 3 }]);
log(Array.from({ length: 4 }, (_, i) => ({ id: i, name: "n" + i, ok: i % 2 === 0 })));

// Own toJSON on a literal and on a nested literal (with its key argument).
log({ a: 1, toJSON() { return "own"; } });
log({ x: { toJSON(k: string) { return "key:" + k; } } });

// Inherited through Object.setPrototypeOf: single, arrays, nested, parsed.
const proto = { toJSON() { return "P"; } };
const a = { a: 1 }; Object.setPrototypeOf(a, proto);
const b = { a: 1 }; Object.setPrototypeOf(b, proto);
log(a); log([a]); log([a, a]); log([a, b]); log([{ a: 1 }, a]); log([a, { a: 1 }]); log({ n: a });
const p = JSON.parse('{"a":1}'); Object.setPrototypeOf(p, proto);
log([p]); log([p, p]); log({ x: [p, p] }); log([JSON.parse('{"a":1}'), p]);

// Inherited through Object.create.
const viaCreate = Object.create(proto); viaCreate.a = 1;
log(viaCreate); log({ n: viaCreate }); log([viaCreate, viaCreate]);

// A class prototype toJSON added after instances were already serialized.
class C { a = 1; b = "two"; }
log(new C()); log([new C(), new C()]);
(C.prototype as any).toJSON = function () { return "C"; };
log(new C()); log([new C(), new C()]); log({ c: new C() });

// Object.prototype.toJSON installed and removed between calls.
(Object.prototype as any).toJSON = function () { return "OP"; };
log({ a: 1 }); log([{ a: 1 }, { a: 2 }]); log({ a: { b: 1 } }); log(JSON.parse('[{"a":1},{"a":2}]'));
delete (Object.prototype as any).toJSON;
log({ a: 1 }); log([{ a: 1 }, { a: 2 }]); log({ a: { b: 1 } });

// An own toJSON added to a literal after it was serialized once.
const late: any = { a: 1 };
log(late); late.toJSON = () => "late"; log(late); log({ m: late }); log([late, late]);

// A toJSON accessor is read once per value (SerializeJSONProperty step 2).
let reads = 0;
const g = { get toJSON() { reads++; return () => "g"; } };
log({ g }); log([g]); log(g);
console.log("toJSON reads", reads);

// The same literal reached twice is not a cycle, and replacers still run.
const shared = { s: 1 };
log({ a: shared, b: shared });
console.log(JSON.stringify({ a: 1, b: { c: 2 } }, (k, v) => (k === "c" ? 3 : v)));
console.log(JSON.stringify({ a: 1, b: [1, 2] }, null, 2));
