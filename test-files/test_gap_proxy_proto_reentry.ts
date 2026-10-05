function read(o: any, key: any): any { return o[key]; }
const base: any = Object.create(null);
base.inheritedLongName = 17;
let calls = 0;
Object.defineProperty(base, "getter", { get: function () {
  calls++;
  return this.marker + read(this, "inheritedLongName");
}});
let traps = 0;
let trapReceiver = false;
const proxy: any = new Proxy(base, { get: function (target: any, key: any, receiver: any) {
  traps++;
  trapReceiver = receiver === child;
  return Reflect.get(target, key, receiver);
}});
const child: any = Object.create(proxy);
child.marker = 9;
console.log("proxy-getter", read(child, "getter"), traps, calls, trapReceiver);
const readingProxy: any = new Proxy(base, { get: function (target: any, key: any, receiver: any) {
  if (key === "outer") return read(receiver, "inheritedLongName");
  return Reflect.get(target, key, receiver);
}});
const readingChild: any = Object.create(readingProxy);
console.log("nested-trap", read(readingChild, "outer"));
const transparent: any = Object.create(new Proxy(readingProxy, {}));
console.log("transparent", read(transparent, "outer"));
