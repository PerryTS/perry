const fromPairs = new Headers([
  ["b", "2"],
  ["a", "1"],
  [1, 2],
] as any);

console.log("pairs:", JSON.stringify(Array.from(fromPairs.entries())));
console.log("coerced:", fromPairs.get("1"));

const cloned = new Headers(fromPairs);
fromPairs.set("a", "changed");
console.log("clone stable:", cloned.get("a"), fromPairs.get("a"));

const fromMap = new Headers(
  new Map<any, any>([
    ["x", "10"],
    ["y", 20],
  ]),
);
console.log("map:", JSON.stringify(Array.from(fromMap.entries())));

const record = new Headers({ Zed: 9, Alpha: "true" } as any);
console.log("record:", JSON.stringify(Array.from(record.entries())));

try {
  new Headers([["ok"]] as any);
} catch (err) {
  console.log("bad pair:", err instanceof TypeError, String(err).includes("Headers"));
}

try {
  new Headers(["ab"] as any);
} catch (err) {
  console.log("string pair:", err instanceof TypeError, String(err).includes("Headers"));
}

try {
  new Headers(null as any);
} catch (err) {
  console.log("null init:", err instanceof TypeError, String(err).includes("Headers"));
}
