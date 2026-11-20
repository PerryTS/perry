const a = Symbol("a"), b = Symbol("b"), c = Symbol("c");
const obj: any = { "10": 10, "2": 2, first: 1 };
function order(o: any): string {
  return Reflect.ownKeys(o).map(k => typeof k === "symbol" ? String(k) : k).join("|");
}
function check(label: string) {
  const desc: any = Object.getOwnPropertyDescriptors(obj);
  const clone: any = Object.create(null, desc);
  const defined: any = Object.defineProperties({}, desc);
  const assigned: any = Object.assign({}, obj);
  console.log(label, order(obj), Object.getOwnPropertySymbols(obj).map(String).join("|"));
  console.log("copies", order(desc), order(clone), order(defined), order(assigned));
  console.log("values", obj[a], clone[a], defined[b], assigned[b]);
}
check("empty");
obj[a] = 11;
Object.defineProperty(obj, b, { get() { return 22; }, enumerable: true, configurable: true });
Object.defineProperty(obj, c, { value: 33, configurable: true });
check("added");
delete obj[a]; obj[a] = 44;
check("readded");
delete obj[a]; delete obj[b]; delete obj[c];
check("removed");
// An earlier immutable string-only prefix must stay empty when a sibling
// extends its canonical key list with a symbol.
const plain: any = { p: 1 };
const sibling: any = { p: 2 }; sibling[a] = 3;
console.log("prefix", Object.getOwnPropertySymbols(plain).length, order(sibling));
console.log("fresh-empty", Object.getOwnPropertySymbols(plain) !== Object.getOwnPropertySymbols(plain));
