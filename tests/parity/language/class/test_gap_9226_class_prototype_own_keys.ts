// #9226: declared class prototypes must expose one coherent
// [[OwnPropertyKeys]] surface. Accessors used to be absent from the string
// half, the internal `@@iterator` dispatch alias leaked as a string key, and
// the real Symbol.iterator key was absent from the symbol half.
//
// Keep the order visible: integer-index strings first (ascending), then other
// strings in property-creation order, then symbols in property-creation order.

const baseSymbol = Symbol("base");
const ownSymbol = Symbol("own");
const staticSymbol = Symbol("static");

function renderKey(key: PropertyKey): string {
  return typeof key === "symbol" ? String(key) : key;
}

function renderKeys(keys: PropertyKey[]): string {
  return keys.map(renderKey).join("|");
}

function sameKeys(left: PropertyKey[], right: PropertyKey[]): boolean {
  if (left.length !== right.length) return false;
  for (let i = 0; i < left.length; i++) {
    if (left[i] !== right[i]) return false;
  }
  return true;
}

function dumpOwnKeys(label: string, value: object, skipFunctionIntrinsics = false): void {
  const names = Object.getOwnPropertyNames(value);
  const symbols = Object.getOwnPropertySymbols(value);
  const reflected = Reflect.ownKeys(value);
  const reflectedNames = reflected.filter((key: PropertyKey) => typeof key === "string");
  const reflectedSymbols = reflected.filter((key: PropertyKey) => typeof key === "symbol");

  console.log(label + ".names=" + renderKeys(names));
  console.log(label + ".symbols=" + renderKeys(symbols));
  console.log(label + ".reflect=" + renderKeys(reflected));
  console.log(
    label + ".partition=" +
      sameKeys(names, reflectedNames) + "," +
      sameKeys(symbols, reflectedSymbols) + "," +
      (reflected.length === names.length + symbols.length),
  );

  // The consistency triple: every reported key must be an own property and
  // must have an own descriptor. This catches a symbol-side hasOwn mismatch
  // independently of list contents. Class-constructor length/name/prototype
  // descriptor parity is tracked separately from #9226, so the static-member
  // pass deliberately checks every user key while leaving those three alone.
  for (const key of reflected) {
    if (
      skipFunctionIntrinsics &&
      (key === "length" || key === "name" || key === "prototype")
    ) continue;
    console.log(
      label + ".key=" + renderKey(key) + ":" +
        Object.prototype.hasOwnProperty.call(value, key) + "," +
        (Object.getOwnPropertyDescriptor(value, key) !== undefined),
    );
  }
}

class Base {
  baseMethod(): string { return "base"; }
  get baseGet(): number { return 1; }
  set baseSet(_value: number) {}
  [baseSymbol](): string { return "base-symbol"; }
}

class Subject extends Base {
  zed(): string { return "zed"; }
  get alpha(): number { return 2; }
  set charlie(_value: number) {}
  get pair(): number { return 3; }
  set pair(_value: number) {}
  10(): string { return "ten"; }
  2(): string { return "two"; }
  [Symbol.iterator](): any { return [][Symbol.iterator](); }
  [ownSymbol](): string { return "own-symbol"; }
  yankee(): string { return "yankee"; }

  static staticZed(): string { return "static-zed"; }
  static get staticAlpha(): number { return 4; }
  static set staticCharlie(_value: number) {}
  static get staticPair(): number { return 5; }
  static set staticPair(_value: number) {}
  static [Symbol.iterator](): any { return [][Symbol.iterator](); }
  static [staticSymbol](): string { return "static-symbol"; }
  static staticYankee(): string { return "static-yankee"; }
}

dumpOwnKeys("base.prototype", Base.prototype);
dumpOwnKeys("subject.prototype", Subject.prototype);
dumpOwnKeys("subject.static", Subject, true);

// Accessors are ordinary own properties even though their values live in
// getter/setter slots rather than in data fields.
for (const key of ["alpha", "charlie", "pair"]) {
  const descriptor = Object.getOwnPropertyDescriptor(Subject.prototype, key);
  console.log(
    "accessor." + key + "=" +
      Object.prototype.hasOwnProperty.call(Subject.prototype, key) + "," +
      (descriptor !== undefined) + "," +
      (descriptor !== undefined && typeof descriptor.get === "function") + "," +
      (descriptor !== undefined && typeof descriptor.set === "function"),
  );
}

// Inherited members remain discoverable through ordinary lookup, but are not
// own keys of the subclass prototype.
for (const key of ["baseMethod", "baseGet", "baseSet"]) {
  console.log(
    "inherited." + key + "=" +
      Object.prototype.hasOwnProperty.call(Subject.prototype, key) + "," +
      (Object.getOwnPropertyDescriptor(Subject.prototype, key) !== undefined),
  );
}
console.log(
  "inherited.symbol=" +
    Object.prototype.hasOwnProperty.call(Subject.prototype, baseSymbol) + "," +
    (Object.getOwnPropertyDescriptor(Subject.prototype, baseSymbol) !== undefined),
);
