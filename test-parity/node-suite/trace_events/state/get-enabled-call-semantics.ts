import { getEnabledCategories } from "node:trace_events";

console.log("plain:", String(getEnabledCategories()));
console.log("with argument:", String((getEnabledCategories as any)("ignored")));
console.log(
  "with receiver:",
  String(getEnabledCategories.call({ ignored: true })),
);
