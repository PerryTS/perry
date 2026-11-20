// BigInt.asIntN / BigInt.asUintN convert their second argument with ToBigInt
// in every program, whichever runtime groups it links. This program names no
// typed array, Buffer or byte module, so the typed-array group is not linked;
// the answers must still be node's (they were 0n before).
function show(label: string, f: () => unknown) {
  try {
    console.log(label, String(f()));
  } catch (e) {
    console.log(label, (e as Error).name);
  }
}
const asIntN = BigInt.asIntN;
const asUintN = BigInt.asUintN;

// Positive cases: BigInt, Boolean, String, object via ToPrimitive, Array via join.
show("int 8 255n", () => asIntN(8, 255n));
show("uint 8 -1n", () => asUintN(8, -1n));
show("int 64 2^63", () => asIntN(64, 9223372036854775808n));
show("uint 64 -1n", () => asUintN(64, -1n));
show("int 0 5n", () => asIntN(0, 5n));
show("int 200 -3n", () => asIntN(200, -3n));
show("int 8 true", () => asIntN(8, true as any));
show("uint 8 false", () => asUintN(8, false as any));
show("int 8 '200'", () => asIntN(8, "200" as any));
show("uint 4 ' 0x1f '", () => asUintN(4, " 0x1f " as any));
show("int 8 ''", () => asIntN(8, "" as any));
show("int 8 valueOf", () => asIntN(8, { valueOf: () => 130n } as any));
show("uint 8 toString", () => asUintN(8, { toString: () => "257" } as any));
show("int 8 toPrimitive", () => asIntN(8, { [Symbol.toPrimitive]: () => -129n } as any));
show("int 8 [300n]", () => asIntN(8, [300n] as any));
show("int 8 []", () => asIntN(8, [] as any));
show("uint 8 Object(9n)", () => asUintN(8, Object(9n)));
show("bits object", () => asIntN({ valueOf: () => 4 } as any, 15n));
show("bits string", () => asUintN("3" as any, 15n));

// Negative controls: what ToBigInt / ToIndex reject still throws node's error.
show("number", () => asIntN(8, 5 as any));
show("double", () => asUintN(8, 1.5 as any));
show("undefined", () => asIntN(8, undefined as any));
show("null", () => asUintN(8, null as any));
show("symbol", () => asIntN(8, Symbol("s") as any));
show("bad string", () => asIntN(8, "12x" as any));
show("plain object", () => asUintN(8, {} as any));
show("array of two", () => asIntN(8, [1, 2] as any));
show("valueOf number", () => asIntN(8, { valueOf: () => 3 } as any));
show("negative bits", () => asIntN(-1, 1n));
show("huge bits", () => asUintN(2 ** 53, 1n));

// The method form and the namespace form agree.
console.log("forms", BigInt.asIntN(8, 255n) === asIntN(8, 255n), BigInt.asUintN(3, 9n));
