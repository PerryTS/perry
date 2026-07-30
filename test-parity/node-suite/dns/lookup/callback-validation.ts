import dns from "node:dns";

function shape(fn: () => unknown): string {
  try {
    fn();
    return "no throw";
  } catch (error: any) {
    return `${error.name}/${error.code}`;
  }
}

console.log("missing:", shape(() => (dns.lookup as any)("127.0.0.1")));
console.log(
  "family overload:",
  shape(() => (dns.lookup as any)("127.0.0.1", 4)),
);
console.log("null:", shape(() => (dns.lookup as any)("127.0.0.1", null)));
console.log(
  "options null callback:",
  shape(() => (dns.lookup as any)("127.0.0.1", {}, null)),
);
