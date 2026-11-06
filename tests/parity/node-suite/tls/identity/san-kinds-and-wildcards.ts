import tls from "node:tls";

function result(host: string, subjectaltname: string) {
  return (tls.checkServerIdentity(
    host,
    { subjectaltname, subject: {} } as any,
  ) as any)?.code ?? "ok";
}
console.log("uri ignored:", result("a.b.example", "URI:http://a.b.example/"));
console.log("ip cidr rejected:", result("8.8.8.8", "IP Address:8.8.8.0/24"));
console.log("bare wildcard rejected:", result("a.com", "DNS:*"));
console.log("top-level wildcard rejected:", result("a.com", "DNS:*.com"));
console.log("multi-label suffix:", result("a.co.uk", "DNS:*.co.uk"));
console.log("partial leftmost:", result("a-cb.a.com", "DNS:*b.a.com"));
console.log("partial crosses label:", result("a.b.a.com", "DNS:*b.a.com"));
console.log(
  "later san matches:",
  result("a.b.a.com", "DNS:*b.a.com, DNS:a.b.a.com"),
);
