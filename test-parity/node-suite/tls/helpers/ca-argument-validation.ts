import tls from "node:tls";

for (
  const [label, value] of [
    ["number", 1],
    ["null", null],
    ["function", () => {}],
    ["boolean", true],
  ] as const
) {
  try {
    tls.getCACertificates(value as any);
    console.log(label + ": no throw");
  } catch (err: any) {
    console.log(label + ":", err instanceof TypeError, err.code);
  }
}
try {
  tls.getCACertificates("test" as any);
  console.log("value: no throw");
} catch (err: any) {
  console.log("value:", err instanceof TypeError, err.code);
}
