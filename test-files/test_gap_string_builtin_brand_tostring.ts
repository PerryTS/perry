// String()/template/inherited-toString on a built-in collection reports its
// brand, not the bare "[object Object]": an override-less Map/Set/WeakMap/
// WeakSet/Promise (and any Symbol.toStringTag object) inherits
// Object.prototype.toString, which brands it.
console.log(String(new Map()));
console.log(String(new Set()));
console.log(String(new WeakMap()));
console.log(String(new WeakSet()));
console.log(String(Promise.resolve(1)));
console.log(`${new Set([1, 2])}`);
console.log(Object.prototype.toString.call(new Map()));
// Plain objects are unchanged.
console.log(String({}));
console.log(String({ a: 1 }));
// Symbol.toStringTag wins.
const tagged: any = {};
tagged[Symbol.toStringTag] = "Widget";
console.log(String(tagged));
// An own/inherited toString override wins over the brand.
const m: any = new Map();
m.toString = () => "custom-map";
console.log(String(m));
