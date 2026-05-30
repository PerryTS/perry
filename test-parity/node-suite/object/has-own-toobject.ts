function show(label: string, fn: () => unknown) {
  try {
    console.log(label + ":", fn());
  } catch (err: any) {
    console.log(label + ":", err.name);
  }
}

show("own", () => Object.hasOwn({ a: 1 }, "a"));
show("inherited", () => Object.hasOwn(Object.create({ a: 1 }), "a"));
show("null", () => Object.hasOwn(null as any, "a"));
show("undefined", () => Object.hasOwn(undefined as any, "a"));
show("string index number", () => Object.hasOwn("ab" as any, 0));
show("string index string", () => Object.hasOwn("ab" as any, "1"));
show("string length", () => Object.hasOwn("ab" as any, "length"));
show("number primitive", () => Object.hasOwn(1 as any, "toString"));
show("array index", () => Object.hasOwn(["x"] as any, 0));
show("array length", () => Object.hasOwn([] as any, "length"));
