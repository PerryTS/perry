import { createTracing } from "node:trace_events";

const first = createTracing({ categories: ["first"] });
const second = createTracing({ categories: ["second"] });
const Constructor = first.constructor as any;

console.log("identity:", Constructor === second.constructor);
console.log("name/length:", Constructor.name, Constructor.length);
console.log(
  "prototype identity:",
  Constructor.prototype === Object.getPrototypeOf(first),
);

try {
  Constructor(["direct"]);
  console.log("call without new: OK");
} catch (error: any) {
  console.log("call without new:", error.name, String(error.code));
}

try {
  const direct = new Constructor(["direct"]);
  console.log(
    "new instance: OK",
    direct instanceof Constructor,
    direct.categories,
    direct.enabled,
  );
} catch (error: any) {
  console.log("new instance: THROW", error.name, String(error.code));
}
