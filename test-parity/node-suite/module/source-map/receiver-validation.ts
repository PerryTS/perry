import { SourceMap } from "node:module";

const map = new SourceMap({
  version: 3,
  sources: ["a.ts"],
  names: [],
  mappings: "AAAA",
});
const payloadGetter = Object.getOwnPropertyDescriptor(
  SourceMap.prototype,
  "payload",
)!.get!;
const lengthsGetter = Object.getOwnPropertyDescriptor(
  SourceMap.prototype,
  "lineLengths",
)!.get!;
for (
  const [label, action] of [
    ["payload", () => payloadGetter.call({})],
    ["lengths", () => lengthsGetter.call(null)],
    ["entry", () => map.findEntry.call({}, 0, 0)],
    ["origin", () => map.findOrigin.call({}, 1, 1)],
  ] as const
) {
  try {
    action();
    console.log(label, "no throw");
  } catch (error) {
    console.log(label, (error as any).name, (error as any).code ?? "no-code");
  }
}
for (
  const [label, args] of [
    ["undefined", [undefined, undefined]],
    ["strings", ["0", "0"]],
    ["non-finite", [NaN, Infinity]],
    ["negative", [-1, 0]],
  ] as any[]
) {
  try {
    console.log(
      "entry args",
      label,
      JSON.stringify(map.findEntry(...args)),
    );
  } catch (error) {
    console.log(
      "entry args",
      label,
      (error as any).name,
      (error as any).code ?? "no-code",
    );
  }
}
