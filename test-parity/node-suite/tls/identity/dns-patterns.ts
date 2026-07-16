import tls from "node:tls";

function ok(host: string, cert: any) {
  return tls.checkServerIdentity(host, cert) === undefined;
}
console.log("case insensitive:", ok("a.example", { subject: { CN: "A.EXAMPLE" } }));
console.log("trailing dot:", ok("a.example", { subject: { CN: "a.example." } }));
console.log("single wildcard:", ok("api.example.com", { subject: { CN: "*.example.com" } }));
console.log("wildcard depth:", ok("deep.api.example.com", { subject: { CN: "*.example.com" } }));
console.log("partial wildcard:", ok("api.example", { subject: { CN: "a*i.example" } }));
console.log("multiple cn:", ok("second.example", { subject: { CN: ["first.example", "second.example"] } }));
console.log("san overrides cn:", ok("cn.example", { subjectaltname: "DNS:san.example", subject: { CN: "cn.example" } }));
