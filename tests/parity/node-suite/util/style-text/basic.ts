import { styleText } from "node:util";

const forceColors = { validateStream: false };

console.log("typeof", typeof styleText);

for (const [label, format, text] of [
  ["red", "red", "x"],
  ["bold-green", ["bold", "green"], "ok"],
  ["none", "none", "plain"],
  ["reset", "reset", "plain"],
  ["grey", "grey", "plain"],
  ["bgGrey", "bgGrey", "plain"],
  ["strikeThrough", "strikeThrough", "plain"],
  ["swapColors", "swapColors", "plain"],
  ["doubleUnderline", "doubleUnderline", "plain"],
  ["overlined", "overlined", "plain"],
  ["framed", "framed", "plain"],
] as const) {
  console.log(label, JSON.stringify(styleText(format as any, text, forceColors)));
}

console.log("default", JSON.stringify(styleText("red", "x")));
console.log("inner-red-close", JSON.stringify(styleText("red", "a\u001b[39mb", forceColors)));
console.log("inner-bold-close", JSON.stringify(styleText("bold", "a\u001b[22mb", forceColors)));

function logThrow(label: string, fn: () => string) {
  try {
    console.log(label, "ok", JSON.stringify(fn()));
  } catch (err) {
    const e = err as Error & { code?: string };
    console.log(label, "throw", e.name, e.code ?? "no-code", err instanceof TypeError);
  }
}

logThrow("invalid-style", () => styleText("missing", "x", forceColors));
logThrow("invalid-format-number", () => styleText(1 as any, "x", forceColors));
logThrow("invalid-array-element", () => styleText(["red", 1] as any, "x", forceColors));
logThrow("invalid-text", () => styleText("red", 1 as any, forceColors));
logThrow("invalid-validateStream", () =>
  styleText("red", "x", { validateStream: "no" } as any),
);
logThrow("null-options", () => styleText("red", "x", null as any));
