const names = ["structuredClone", "atob", "btoa"];

for (const name of names) {
  const value = globalThis[name];
  const desc = Object.getOwnPropertyDescriptor(globalThis, name);

  console.log(name + " typeof:", typeof value);
  console.log(
    name + " descriptor:",
    desc !== undefined,
    desc ? desc.writable : "missing",
    desc ? desc.enumerable : "missing",
    desc ? desc.configurable : "missing",
  );
  console.log(
    name + " name-length:",
    value ? value.name : "missing",
    value ? value.length : "missing",
  );
}

const clone = globalThis.structuredClone;
const source = { nested: { value: 42 }, list: [1, 2, 3] };
const cloned = clone(source);
console.log(
  "clone values:",
  cloned.nested.value,
  cloned.list.join(","),
  cloned !== source,
  cloned.nested !== source.nested,
);

const decode = globalThis.atob;
const encode = globalThis.btoa;
console.log("base64 values:", decode("cGVycnk="), encode("perry"));
