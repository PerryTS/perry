const u8 = new Uint8Array(4);
let keyCalls = 0;
function nextKey(): number {
  keyCalls++;
  return keyCalls - 1;
}

const firstResult = (u8["p" + nextKey()] = 10);
const secondResult = (u8["p" + nextKey()] = 20);
u8[0] = 3;
const numericStringResult = (u8["1"] = 4);
u8.tag = 30;
const symbolKey = Symbol("hidden");
const symbolResult = (u8[symbolKey] = 40);

console.log(
  JSON.stringify({
    keys: Object.keys(u8),
    values: [u8.p0, u8.p1, u8[0], u8[1], u8.tag, u8[symbolKey]],
    results: [firstResult, secondResult, numericStringResult, symbolResult],
    symbolCount: Object.getOwnPropertySymbols(u8).length,
    keyCalls,
    descriptors: [
      Object.getOwnPropertyDescriptor(u8, "p0"),
      Object.getOwnPropertyDescriptor(u8, "p1"),
    ],
  }),
);
