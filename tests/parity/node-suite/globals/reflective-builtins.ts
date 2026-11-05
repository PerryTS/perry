function show(label: string, fn: () => unknown) {
  try {
    console.log(label, "ok", String(fn()));
  } catch (err: any) {
    console.log(label, "throw", err?.constructor?.name ?? err?.name);
  }
}

const map = new Map();
show("Map.set invalid receiver", () => Map.prototype.set.call(null, "k", "v"));
show("Map.set valid receiver", () => {
  Map.prototype.set.call(map, "k", "v");
  return map.get("k");
});

const weakMap = new WeakMap();
const weakKey = {};
show("WeakMap.set invalid receiver", () => WeakMap.prototype.set.call(undefined, weakKey, 1));
show("WeakMap.set valid receiver", () => {
  WeakMap.prototype.set.call(weakMap, weakKey, 2);
  return weakMap.get(weakKey);
});

show("Number.valueOf invalid receiver", () => Number.prototype.valueOf.call({}));
show("Number.valueOf primitive receiver", () => Number.prototype.valueOf.call(12));
show("Number.toFixed invalid receiver", () => Number.prototype.toFixed.call({}, 1));
show("Number.toFixed primitive receiver", () => Number.prototype.toFixed.call(1.23, 1));
show("Number.toPrecision invalid receiver", () => Number.prototype.toPrecision.call({}, 2));
show("Number.toPrecision primitive receiver", () => Number.prototype.toPrecision.call(1.23, 2));
show("Number.toExponential invalid receiver", () => Number.prototype.toExponential.call({}, 1));
show("Number.toExponential primitive receiver", () => Number.prototype.toExponential.call(1.23, 1));
show("Number.toString invalid receiver", () => Number.prototype.toString.call({}));
show("Number.toString primitive receiver", () => Number.prototype.toString.call(31, 16));
show("Number.toLocaleString invalid receiver", () => Number.prototype.toLocaleString.call({}));

show("Boolean.toString invalid receiver", () => Boolean.prototype.toString.call({}));
show("Boolean.toString primitive receiver", () => Boolean.prototype.toString.call(false));
show("Boolean.valueOf invalid receiver", () => Boolean.prototype.valueOf.call({}));
show("Boolean.valueOf primitive receiver", () => Boolean.prototype.valueOf.call(true));

const symbolValue = Symbol("x");
show("Symbol.toString invalid receiver", () => Symbol.prototype.toString.call({}));
show("Symbol.toString primitive receiver", () => Symbol.prototype.toString.call(symbolValue));
show("Symbol.valueOf invalid receiver", () => Symbol.prototype.valueOf.call({}));
show("Symbol.valueOf primitive receiver", () => Symbol.prototype.valueOf.call(symbolValue) === symbolValue);

const bigintValue = BigInt("31");
show("BigInt.toString invalid receiver", () => BigInt.prototype.toString.call({}));
show("BigInt.toString primitive receiver", () => BigInt.prototype.toString.call(bigintValue, 16));
show("BigInt.valueOf invalid receiver", () => BigInt.prototype.valueOf.call({}));
show("BigInt.valueOf primitive receiver", () => BigInt.prototype.valueOf.call(BigInt("5")) === BigInt("5"));

const ArrayCtor: any = Array;
const ObjectCtor: any = Object;
const DateCtor: any = Date;

show("Array dynamic instanceof", () => [] instanceof ArrayCtor);
show("Object dynamic instanceof", () => ({}) instanceof ObjectCtor);
show("Date dynamic instanceof", () => new Date(0) instanceof DateCtor);

show("Array Symbol.hasInstance", () => ArrayCtor[Symbol.hasInstance]([]));
show("Object Symbol.hasInstance", () => ObjectCtor[Symbol.hasInstance]({}));
show("Date Symbol.hasInstance", () => DateCtor[Symbol.hasInstance](new Date(0)));
show("Function hasInstance inherited", () => typeof ArrayCtor[Symbol.hasInstance]);
