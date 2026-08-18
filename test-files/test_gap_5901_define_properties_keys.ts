// #5901: ObjectDefineProperties must ToObject-box a primitive properties bag,
// collect String and Symbol keys in [[OwnPropertyKeys]] order, and perform the
// descriptor bag's observable [[GetOwnProperty]] / [[Get]] operations.

function outcome(fn: () => void): string {
  try {
    fn();
    return "ok";
  } catch (error: any) {
    return error.name;
  }
}

console.log("primitive-empty", outcome(() => Object.defineProperties({}, true as any)));
console.log("primitive-string", outcome(() => Object.defineProperties({}, "hello" as any)));
console.log("create-string", outcome(() => Object.create({}, "hello" as any)));

const symbolKey = Symbol("descriptor");
const symbolBag: any = {};
symbolBag[symbolKey] = { value: 42, enumerable: true };
const symbolTarget: any = {};
Object.defineProperties(symbolTarget, symbolBag);
console.log("symbol", symbolTarget[symbolKey], Reflect.ownKeys(symbolTarget).length);
const hiddenSymbol = Symbol("hidden");
Object.defineProperty(symbolBag, hiddenSymbol, {
  value: { value: 99 },
  enumerable: false,
});
const hiddenSymbolTarget: any = {};
Object.defineProperties(hiddenSymbolTarget, symbolBag);
console.log("hidden-symbol", Reflect.ownKeys(hiddenSymbolTarget).length);

const proxyLog: PropertyKey[] = [];
const proxyTarget: any = { 0: 1, foo: 2 };
const proxySymbol = Symbol("proxy");
proxyTarget[proxySymbol] = 3;
const proxyBag = new Proxy(proxyTarget, {
  getOwnPropertyDescriptor(_target, key) {
    proxyLog.push(key);
    return undefined;
  },
});
Object.defineProperties({}, proxyBag);
console.log(
  "proxy-order",
  proxyLog.map((key) => typeof key === "symbol" ? key.toString() : key).join("|"),
);
