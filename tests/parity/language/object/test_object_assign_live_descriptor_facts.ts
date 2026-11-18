const events: string[] = [];
const symbol = Symbol("visible");
const source: any = {};
Object.defineProperty(source, "first", {
  enumerable: true,
  get() {
    events.push("get:first");
    Object.defineProperty(source, "hiddenLater", { enumerable: false });
    delete source.deletedLater;
    return 1;
  },
});
source.hiddenLater = 2;
source.deletedLater = 3;
source.valueLater = 4;
source[symbol] = 5;
const target: any = {
  set first(value: number) {
    events.push("set:first:" + value);
    source.valueLater = 40;
    source.addedLater = 6;
  },
};
Object.assign(target, source);
console.log("ordinary", events.join(","), Object.keys(target).join(","));
console.log("values", target.hiddenLater, target.deletedLater, target.valueLater, target[symbol], target.addedLater);

events.length = 0;
const proxied = new Proxy({ first: 7, later: 8 }, {
  ownKeys(object) {
    events.push("keys");
    return Reflect.ownKeys(object);
  },
  getOwnPropertyDescriptor(object, key) {
    events.push("desc:" + String(key));
    return Reflect.getOwnPropertyDescriptor(object, key);
  },
  get(object, key, receiver) {
    events.push("get:" + String(key));
    return Reflect.get(object, key, receiver);
  },
});
const proxyTarget: any = {
  set first(value: number) {
    events.push("set:first:" + value);
    Object.defineProperty(proxied, "later", { enumerable: false });
  },
};
Object.assign(proxyTarget, proxied);
console.log("proxy", events.join(","), Object.hasOwn(proxyTarget, "later"));
