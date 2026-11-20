import dns from "node:dns";
import dnsPromises from "node:dns/promises";

function shape(Resolver: any, options: any): string {
  try {
    new Resolver(options);
    return "ok";
  } catch (error: any) {
    return `${error.name}/${error.code}`;
  }
}

const cases: Array<[string, any]> = [
  ["undefined", undefined],
  ["null", null],
  ["timeout -1", { timeout: -1 }],
  ["timeout zero", { timeout: -0 }],
  ["timeout float", { timeout: 1.5 }],
  ["timeout type", { timeout: "1" }],
  ["tries one", { tries: 1 }],
  ["tries zero", { tries: 0 }],
  ["tries type", { tries: "1" }],
  ["max zero", { maxTimeout: -0 }],
  ["max negative", { maxTimeout: -1 }],
  ["max float", { maxTimeout: 1.5 }],
];

for (const [label, options] of cases) {
  console.log(
    label + ":",
    shape(dns.Resolver, options),
    shape(dnsPromises.Resolver, options),
  );
}
