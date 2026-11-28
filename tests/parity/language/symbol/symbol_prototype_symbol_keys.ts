// Symbol.prototype owns @@toStringTag and @@toPrimitive with the spec
// attributes, and ToPrimitive of a Symbol wrapper reads the latter.
const proto: any = Symbol.prototype;
console.log(Reflect.ownKeys(proto).filter((k) => typeof k === "symbol").map(String));
console.log(Object.getOwnPropertySymbols(proto).map(String));

const tag: any = Object.getOwnPropertyDescriptor(proto, Symbol.toStringTag);
console.log(tag.value, tag.writable, tag.enumerable, tag.configurable);

const prim: any = Object.getOwnPropertyDescriptor(proto, Symbol.toPrimitive);
console.log(typeof prim.value, prim.writable, prim.enumerable, prim.configurable);
console.log(prim.value.name, prim.value.length, "prototype" in prim.value);
for (const key of ["name", "length"]) {
  const d: any = Object.getOwnPropertyDescriptor(prim.value, key);
  console.log(key, d.value, d.writable, d.enumerable, d.configurable);
}
try {
  new prim.value();
  console.log("constructed");
} catch (e) {
  console.log("not a constructor", e instanceof TypeError);
}

const s = Symbol("s");
const anyS: any = s;
const boxed: any = Object(s);
console.log(prim.value.call(s) === s, prim.value.call(boxed) === s);
console.log(prim.value.call(s, "number") === s, prim.value.call(s, {}) === s);
console.log(anyS[Symbol.toPrimitive] === prim.value, boxed[Symbol.toPrimitive]("string") === s);
console.log(anyS[Symbol.toStringTag], boxed[Symbol.toStringTag]);
for (const value of [null, undefined, 0, "", {}, true, [], proto]) {
  try {
    prim.value.call(value);
    console.log("no throw");
  } catch (e) {
    console.log(e instanceof TypeError);
  }
}

const toString = Object.prototype.toString;
console.log(toString.call(s), toString.call(boxed), toString.call(proto));
console.log(toString.call(Object.create(proto)));
console.log(String(s), boxed == s, boxed.toString());

const attempt = (f: () => any) => {
  try {
    return String(f());
  } catch (e) {
    return e instanceof TypeError ? "TypeError" : "other";
  }
};
const plain: any = Object.create(proto);
console.log(attempt(() => `${boxed}`), attempt(() => boxed + ""), attempt(() => +boxed));
console.log(attempt(() => plain + ""), attempt(() => `${plain}`), attempt(() => +plain));

// Neither key is writable.
console.log(Reflect.set(proto, Symbol.toStringTag, "x"), Reflect.set(proto, Symbol.toPrimitive, null));
