// Control for test_bigint_as_n_tobigint: with the typed-array group linked,
// ToBigInt of a typed array (BigInt.asIntN's argument, a BigInt64Array
// element store) still runs the view's own patched valueOf / toString first,
// and the element stores keep their ToBigInt errors.
function show(label: string, f: () => unknown) {
  try {
    console.log(label, String(f()));
  } catch (e) {
    console.log(label, (e as Error).name);
  }
}
const plain = new BigInt64Array([5n]);
show("asIntN plain view", () => BigInt.asIntN(8, plain as any));
const patched: any = new BigInt64Array([5n]);
patched.valueOf = () => 300n;
show("asIntN patched valueOf", () => BigInt.asIntN(8, patched));
const stringy: any = new BigUint64Array(2);
stringy.toString = () => "258";
show("asUintN patched toString", () => BigInt.asUintN(8, stringy));
const pair = new BigInt64Array(2);
show("asIntN two elements", () => BigInt.asIntN(8, pair as any));
const store = new BigInt64Array(4);
show("store bigint", () => ((store[0] = 7n), store[0]));
show("store true", () => ((store[1] = true as any), store[1]));
show("store string", () => ((store[2] = "-9" as any), store[2]));
show("store valueOf", () => ((store[3] = { valueOf: () => 11n } as any), store[3]));
show("store view", () => ((store[0] = patched), store[0]));
show("store number", () => { store[0] = 1 as any; return store[0]; });
show("store undefined", () => { store[0] = undefined as any; return store[0]; });
show("fill symbol", () => store.fill(Symbol("q") as any));
show("asIntN number", () => BigInt.asIntN(8, 1 as any));
console.log("after", store.join(","), BigInt.asIntN(8, 255n), BigInt.asUintN(8, -1n));
