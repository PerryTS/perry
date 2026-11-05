// Which constructor @@split and @@matchAll construct through: the @@species of
// the separator's constructor, read as an observable Get on the constructor.
class Loud extends RegExp {
  exec(s: string): any {
    console.log("Loud exec", this.lastIndex);
    return null;
  }
}
// 1. A static own @@species getter on the subclass is read, once per operation.
class Own extends RegExp {
  static get [Symbol.species]() {
    console.log("species get", this === Own || this === Deep);
    return Loud;
  }
}
console.log(JSON.stringify("abc".split(new Own("b"))));
console.log([...("abc".matchAll(new Own("b", "g")))].length);
// 2. The getter inherited through a subclass is read too.
class Deep extends Own {}
console.log(JSON.stringify("abc".split(new Deep("b"))));
// 3. @@species undefined: the intrinsic RegExp and its builtin exec.
class NoSpecies extends RegExp {
  static get [Symbol.species]() {
    return undefined;
  }
}
console.log(JSON.stringify("abc".split(new NoSpecies("b"))));
// 4. An ordinary object as `constructor` holding @@species.
const re: any = /b/;
re.constructor = { [Symbol.species]: Loud };
console.log(JSON.stringify("abc".split(re)));
// 5. A proxy constructor: its get trap observes the @@species read.
class Trapped extends RegExp {}
const seen: string[] = [];
const proxied = new Proxy(Trapped, {
  get(target: any, key: any, receiver: any) {
    seen.push(typeof key === "symbol" ? String(key.description) : String(key));
    return Reflect.get(target, key, receiver);
  },
});
const trapped: any = new Trapped("b");
trapped.constructor = proxied;
console.log(JSON.stringify("abc".split(trapped)), seen.includes("Symbol.species"));
