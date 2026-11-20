function showObject(label: string, build: () => any) {
  try {
    const obj = build();
    console.log(label, "ok", JSON.stringify(Object.entries(obj)));
  } catch (err: any) {
    console.log(
      label,
      "throw",
      err.name,
      String(err.message).includes("iterable"),
      String(err.message).includes("entry object"),
    );
  }
}

const customIterable = {
  [Symbol.iterator]() {
    return [
      ["c", 5],
      { 0: "d", 1: 6 },
    ][Symbol.iterator]();
  },
};

showObject("array", () => Object.fromEntries([["a", 1], ["b", 2]]));
showObject("map", () => Object.fromEntries(new Map([["m", 3]])));
showObject("set", () => Object.fromEntries(new Set<any>([["s", 4]])));
showObject("custom", () => Object.fromEntries(customIterable as any));
showObject("short", () => Object.fromEntries([["short"] as any]));
showObject("empty-pair", () => Object.fromEntries([[] as any]));
showObject("null-input", () => Object.fromEntries(null as any));
showObject("undefined-input", () => Object.fromEntries(undefined as any));
showObject("number-input", () => Object.fromEntries(1 as any));
showObject("plain-object-input", () => Object.fromEntries({ a: 1 } as any));
showObject("non-object-entry", () => Object.fromEntries([1 as any]));
