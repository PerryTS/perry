const g: any = globalThis;

console.log(
  "direct typeof:",
  typeof globalThis.structuredClone,
  typeof globalThis.atob,
  typeof globalThis.btoa,
);

for (const name of ["structuredClone", "atob", "btoa"]) {
  const fn = g[name];
  const desc = Object.getOwnPropertyDescriptor(globalThis, name);
  console.log(`${name} typeof:`, typeof fn);
  console.log(`${name} name/length:`, fn?.name, fn?.length);
  console.log(
    `${name} desc:`,
    !!desc,
    desc?.writable,
    desc?.enumerable,
    desc?.configurable,
  );
}

const original = { nested: { value: 7 } };
const clone = g.structuredClone;
const cloned = clone(original);
console.log("structuredClone rebound distinct:", cloned !== original);
console.log(
  "structuredClone rebound nested:",
  cloned.nested !== original.nested,
  cloned.nested.value,
);

const encode = g.btoa;
const decode = g.atob;
console.log("btoa rebound:", encode("perry"));
console.log("atob rebound:", decode("cGVycnk="));
console.log("roundtrip rebound:", decode(encode("Hello")));
