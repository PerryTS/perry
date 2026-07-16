import { WASI } from "node:wasi";

const W: any = WASI;

for (
  const [label, create] of [
    ["missing options", () => new W()],
    ["empty options", () => new W({})],
    ["undefined version", () => new W({ version: undefined })],
    ["numeric version", () => new W({ version: 1 })],
    ["unknown version", () => new W({ version: "preview2" })],
    ["preview1", () => new W({ version: "preview1" })],
    ["unstable", () => new W({ version: "unstable" })],
  ] as const
) {
  try {
    const value = create();
    console.log(label + ": ok", value instanceof WASI);
  } catch (error: any) {
    console.log(label + ": throw", error?.name, error?.code || "no-code");
  }
}
