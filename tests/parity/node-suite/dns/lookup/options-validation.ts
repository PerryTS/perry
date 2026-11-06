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

const cases: Array<[string, any]> = [
  ["options", "6"],
  ["family", { family: "6" }],
  ["hints type", { hints: "0" }],
  ["hints value", { hints: -1 }],
  ["all", { all: 1 }],
  ["verbatim", { verbatim: "true" }],
  ["order", { order: "true" }],
];

for (const [label, options] of cases) {
  console.log(
    label + ":",
    shape(() => dns.lookup("127.0.0.1", options, () => {})),
    shape(() => dnsPromises.lookup("127.0.0.1", options)),
  );
}
