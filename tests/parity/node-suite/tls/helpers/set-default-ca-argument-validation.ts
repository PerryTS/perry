import tls from "node:tls";
import { readFileSync } from "node:fs";

const cert = readFileSync(
  new URL("../fixtures/localhost-cert.pem", import.meta.url),
).toString();
tls.setDefaultCACertificates([cert]);

function unchanged() {
  const actual = tls.getCACertificates("default");
  return actual.length === 1 && actual[0] === cert;
}
for (
  const [label, value] of [
    ["null", null],
    ["undefined", undefined],
    ["string", "string"],
    ["number", 42],
    ["object", {}],
    ["boolean", true],
  ] as const
) {
  try {
    tls.setDefaultCACertificates(value as any);
    console.log(label + ": no throw", unchanged());
  } catch (err: any) {
    console.log(label + ":", err instanceof TypeError, err.code, unchanged());
  }
}
for (
  const [label, value] of [["null", null], ["number", 42], ["object", {}], [
    "boolean",
    true,
  ]] as const
) {
  for (
    const [position, input] of [["first", [value]], ["second", [
      cert,
      value,
    ]]] as const
  ) {
    try {
      tls.setDefaultCACertificates(input as any);
      console.log(label + " " + position + ": no throw", unchanged());
    } catch (err: any) {
      console.log(
        label + " " + position + ":",
        err instanceof TypeError,
        err.code,
        unchanged(),
      );
    }
  }
}
