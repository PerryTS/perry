import dns from "node:dns";
import dnsPromises from "node:dns/promises";

function shape(fn: () => unknown): string {
  try {
    fn();
    return "no throw";
  } catch (error: any) {
    return `${error.name}/${error.code}`;
  }
}

const callbackCases: Array<[string, () => unknown]> = [
  ["missing", () => (dns.lookupService as any)("127.0.0.1")],
  ["address", () => (dns.lookupService as any)("localhost", 80, () => {})],
  ["port low", () => (dns.lookupService as any)("127.0.0.1", -1, () => {})],
  ["port high", () => (dns.lookupService as any)("127.0.0.1", 65536, () => {})],
  [
    "port text",
    () => (dns.lookupService as any)("127.0.0.1", "nope", () => {}),
  ],
  ["callback", () => (dns.lookupService as any)("127.0.0.1", 80, null)],
];

for (const [label, fn] of callbackCases) {
  console.log("callback " + label + ":", shape(fn));
}
for (
  const [label, address, port] of [
    ["missing", "127.0.0.1", undefined],
    ["address", "localhost", 80],
    ["port low", "127.0.0.1", -1],
    ["port high", "127.0.0.1", 65536],
    ["port text", "127.0.0.1", "nope"],
  ] as const
) {
  console.log(
    "promise " + label + ":",
    shape(() => (dnsPromises.lookupService as any)(address, port)),
  );
}
