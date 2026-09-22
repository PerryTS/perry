// #10480, sloppy half: in non-strict CJS the broken define did not throw — the
// write was silently dropped, which is how it reached node-fetch's
// `Request.prototype` and whatwg-url's `URL.prototype` without an error.
// An attributes-only descriptor must leave the setter in place, so the write
// still runs it here and the getter reports the setter's value.

class Sloppy {
  _p = "";
  get pathname() {
    return this._p;
  }
  set pathname(value: string) {
    this._p = "set:" + value;
  }
  get readOnly() {
    return "ro";
  }
}

Object.defineProperties(Sloppy.prototype, {
  pathname: { enumerable: true },
  readOnly: { enumerable: true },
});

const instance = new Sloppy();
instance.pathname = "/sloppy";
console.log("assigned", instance.pathname, instance._p);

const dynamic: any = new Sloppy();
const key = "pathname";
dynamic[key] = "/dynamic";
console.log("dynamic", dynamic[key]);

console.log("read-only", (instance as any).readOnly);

const descriptor = Object.getOwnPropertyDescriptor(Sloppy.prototype, "pathname");
console.log("descriptor", typeof descriptor.get, typeof descriptor.set, descriptor.enumerable, descriptor.configurable);

const keys: string[] = [];
for (const name in instance) keys.push(name);
console.log("for-in", keys.join(","));
console.log("keys", Object.keys(Sloppy.prototype).join(","));
