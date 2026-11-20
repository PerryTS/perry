// Charter step 3, SLOPPY mode: a store the attributes forbid fails SILENTLY
// here (strict mode throws; see test_gap_attrs_in_shape.ts). Every site is
// primed before the attribute changes.

function out(label: string, v: unknown): void {
  console.log(label + ": " + (typeof v === "string" ? v : JSON.stringify(v)));
}

function setA(o: any, v: number): void {
  o.a = v;
}
function addK(o: any, i: number): void {
  o["k" + i] = i;
}

const plain: any = { a: 1, b: 2 };
const ro: any = { a: 1, b: 2 };
const frozen: any = { a: 1, b: 2 };
const sealed: any = { a: 1, b: 2 };
const noext: any = { a: 1, b: 2 };
const all = [plain, ro, frozen, sealed, noext];
for (let i = 0; i < 400; i++) for (const o of all) setA(o, i);

Object.defineProperty(ro, "a", { writable: false });
Object.freeze(frozen);
Object.seal(sealed);
Object.preventExtensions(noext);

// Several stores each: a miss that primes the site must not let a LATER
// store through the inline path.
for (let r = 0; r < 3; r++) for (const o of all) setA(o, 1000 + r);
for (const o of all) addK(o, 7);
out("a", all.map((o) => o.a));
out("keys", all.map((o) => Object.keys(o).join("|")));
out("delete-b", all.map((o) => delete o.b));
out("has-b", all.map((o) => Object.prototype.hasOwnProperty.call(o, "b")));

let got = 0;
const acc: any = { v: 1 };
for (let i = 0; i < 300; i++) got += acc.v;
Object.defineProperty(acc, "v", { get: () => 5, configurable: true });
acc.v = 77; // getter-only accessor: silently ignored
got += acc.v;
out("getter-only", got);
