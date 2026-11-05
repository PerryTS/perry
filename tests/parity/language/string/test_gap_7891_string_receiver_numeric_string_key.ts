// #7891: an erased array annotation must not make a numeric string key
// interpret a runtime string receiver as an ArrayHeader.

type Bag = { items: string[] };

function mk(value: any): Bag {
  return { items: value };
}

function viaDeclared(bag: Bag): string {
  return "" + bag.items["0"] + "/" + bag.items[0];
}

function viaDynamicKey(bag: Bag, key: string): string {
  return "" + bag.items[key];
}

function viaNullable(bag: Bag | null): string {
  if (bag === null) return "null";
  return "" + bag.items["0"];
}

const stringValue: any = "ss";
console.log(viaDeclared(mk(stringValue)));
console.log(viaDynamicKey(mk(stringValue), "1"));
console.log("" + stringValue["length"] + "/" + typeof stringValue["constructor"]);
console.log(viaNullable(mk(stringValue)));

// JSON.parse produces an inline short string on Perry, exercising the second
// receiver representation that cannot survive an ArrayHeader unbox at all.
const shortStringValue: any = JSON.parse('{"value":"ss"}').value;
console.log(viaDeclared(mk(shortStringValue)));
console.log(viaDynamicKey(mk(shortStringValue), "1"));

// Keep the ordinary array arm and the direct, runtime-classified string arm
// beside the violated declaration so the expected behavior is unambiguous.
console.log(viaDeclared(mk(["a", "b"])));
console.log(viaDynamicKey(mk(["a", "b"]), "1"));
const direct: any = "ss";
console.log("" + direct["0"] + "/" + direct[0]);

// The same false claim may hold an ordinary object; its established property
// lookup must remain unchanged by the String-specific receiver guard.
console.log(viaDeclared(mk({ 0: "object" })));
