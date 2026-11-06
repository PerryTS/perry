class LocalFlags extends RegExp {
  get flags() {
    return this instanceof LocalFlags ? "" : "wrong receiver";
  }
}

const re = new LocalFlags("\\+", "g");
console.log("flags:", re.flags);
console.log("reflect:", Reflect.get(LocalFlags.prototype, "flags", re));
console.log("prototype:", Object.getPrototypeOf(re) === LocalFlags.prototype);
console.log("replace:", "a+b+c".replace(re, "-"));

class LocalSource extends RegExp {
  get source() { return "override"; }
}
console.log("source:", new LocalSource("a").source);

Object.defineProperty(re, "flags", { value: "own", configurable: true });
console.log("own flags:", re.flags);
delete (re as any).flags;
Object.defineProperty(LocalFlags.prototype, "flags", { value: "prototype data", configurable: true });
console.log("prototype data:", re.flags);
console.log("reflected data:", Reflect.get(LocalFlags.prototype, "flags", re));
