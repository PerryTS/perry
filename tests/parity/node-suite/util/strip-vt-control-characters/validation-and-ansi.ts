import { stripVTControlCharacters as strip } from "node:util";

for (const input of [
  "a\x1b[31mb",
  "a\x9b31mb",
  "a\x1b]0;title\x07b",
  "a\x1b]0;title\x1b\\b",
  "a\x1b7b",
  "a\x1b(Bb",
  "a\x07b",
]) {
  console.log(JSON.stringify(strip(input)));
}

for (const [label, value] of [
  ["number", 123],
  ["null", null],
  ["undefined", undefined],
  ["object", { toString() { return "\x1b[31mx"; } }],
] as const) {
  try {
    console.log(label, "ok", JSON.stringify(strip(value as any)));
  } catch (err) {
    const e = err as Error & { code?: string };
    console.log(
      label,
      "throw",
      e.name,
      e.code ?? "no-code",
      e.message.includes('"str"'),
      err instanceof TypeError,
    );
  }
}
