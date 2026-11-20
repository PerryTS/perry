import tls from "node:tls";

function result(host: string, san?: string, cn?: string) {
  const err = tls.checkServerIdentity(host, { subjectaltname: san, subject: cn ? { CN: cn } : {} } as any);
  return err ? [err.code, err.host, err.reason].join("/") : "ok";
}
console.log("ipv4 san:", result("127.0.0.1", "IP Address:127.0.0.1"));
console.log("ipv4 dns rejected:", result("127.0.0.1", "DNS:127.0.0.1", "127.0.0.1"));
console.log("ipv4 cn rejected:", result("127.0.0.1", undefined, "127.0.0.1"));
console.log("ipv4 mismatch:", result("127.0.0.2", "IP Address:127.0.0.1"));
console.log("ipv6 canonical:", result("::1", "IP Address:0:0:0:0:0:0:0:1"));
