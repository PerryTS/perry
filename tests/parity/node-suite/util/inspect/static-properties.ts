import util, { inspect } from "node:util";

const captured = inspect;

console.log("custom eq:", inspect.custom === Symbol.for("nodejs.util.inspect.custom"));
console.log("namespace same:", util.inspect.custom === inspect.custom);
console.log("captured same:", captured.custom === inspect.custom);

console.log(
  "default options:",
  typeof inspect.defaultOptions,
  inspect.defaultOptions.depth,
  inspect.defaultOptions.customInspect,
  inspect.defaultOptions.colors,
);
console.log("styles:", typeof inspect.styles, inspect.styles.number, inspect.styles.special);
console.log(
  "colors:",
  Array.isArray(inspect.colors.yellow),
  inspect.colors.yellow.join(","),
  inspect.colors.green.join(","),
);

const originalDepth = inspect.defaultOptions.depth;
inspect.defaultOptions.depth = 0;
console.log("depth default:", inspect({ a: { b: 1 } }));
inspect.defaultOptions.depth = originalDepth;

const originalCustomInspect = inspect.defaultOptions.customInspect;
const hooked: any = { [inspect.custom]: () => "CUSTOM" };
inspect.defaultOptions.customInspect = false;
console.log("custom off has symbol:", inspect(hooked).includes("Symbol(nodejs.util.inspect.custom)"));
inspect.defaultOptions.customInspect = originalCustomInspect;
console.log("custom restored:", inspect(hooked));
