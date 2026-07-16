import tls from "node:tls";

function ok(host: string, name: string) {
  return tls.checkServerIdentity(host, { subject: { CN: name } } as any) === undefined;
}
function code(host: string, name: string) {
  return (tls.checkServerIdentity(host, { subject: { CN: name } } as any) as any)?.code;
}
console.log("idna wildcard:", ok("xn--bcher-kva.example.com", "*.example.com"));
console.log("idna embedded wildcard:", code("xn--bcher-kva.example.com", "xn--*.example.com"));
console.log("empty label:", code("bad.x.example.com", "bad..example.com"));
console.log("space label:", code("x.example.com", "bad label.com"));
console.log("unicode label:", code("x.example.com", "café.example.com"));
console.log("unicode separator:", code("foo。bar.example.com", "*.example.com"));
