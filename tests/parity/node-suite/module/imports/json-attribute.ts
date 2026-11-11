import data from "../require/fixtures/data.json" with { type: "json" };

console.log("json:", data.name, data.count, data.nested.ok);
console.log(
  "frozen/extensible:",
  Object.isFrozen(data),
  Object.isExtensible(data),
);
