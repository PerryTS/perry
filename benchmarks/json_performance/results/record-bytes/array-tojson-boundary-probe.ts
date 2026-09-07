function show(label: string, value: any) {
  try { console.log(label, JSON.stringify(value)); }
  catch (e) { console.log(label, "throws"); }
}
(Object.prototype as any).toJSON = function (key: string) { return "object:" + key + ":" + this.length; };
show("object-prototype", [1, 2]);
const shadow: any = [1, 2]; shadow.toJSON = undefined;
show("own-undefined-shadows", shadow);
const noPrototype: any = [1, 2]; Object.setPrototypeOf(noPrototype, null);
show("null-prototype", noPrototype);
delete (Object.prototype as any).toJSON;
(Array.prototype as any).toJSON = function (key: string) { return "array:" + key + ":" + this.length; };
show("array-prototype", [1, 2]);
show("array-property-key", { longPropertyKey: [1, 2] });
delete (Array.prototype as any).toJSON;
const custom: any = [1, 2];
Object.setPrototypeOf(custom, { toJSON(key: string) { return "custom:" + key + ":" + this.length; } });
show("custom-prototype", custom);
let gets = 0;
const ownGetter: any = [1, 2];
Object.defineProperty(ownGetter, "toJSON", { configurable: true, get() { gets++; return function (key: string) { return "getter:" + key + ":" + this.length; }; } });
show("own-getter", ownGetter);
console.log("own-getter-count", gets);
Object.defineProperty(Object.prototype, "toJSON", { configurable: true, get() { gets++; return function (key: string) { return "inherited-getter:" + key + ":" + this.length; }; } });
show("inherited-getter", [1, 2]);
console.log("inherited-getter-count", gets);
delete (Object.prototype as any).toJSON;
show("after-reset", [1, 2]);
