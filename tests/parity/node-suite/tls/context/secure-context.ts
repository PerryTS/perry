import tls from "node:tls";

const ctx = tls.createSecureContext();
console.log("exports:", typeof tls.createSecureContext === "function" && typeof tls.SecureContext === "function");
console.log("context shape:", typeof ctx === "object" &&
  ctx.constructor === tls.SecureContext &&
  ctx instanceof tls.SecureContext &&
  typeof ctx.context === "object" &&
  Object.keys(ctx).join(",") === "context");
console.log("prototype link:", tls.SecureContext.prototype.constructor === tls.SecureContext &&
  Object.getPrototypeOf(ctx) === tls.SecureContext.prototype);

const constructed = new tls.SecureContext({ minVersion: "TLSv1.2", maxVersion: "TLSv1.3" });
console.log("new secure context:", constructed instanceof tls.SecureContext && typeof constructed.context === "object");

try {
  tls.createSecureContext({ minVersion: "TLSv1.4" as any });
  console.log("invalid minVersion: no throw");
} catch (err: any) {
  console.log("invalid minVersion:", err instanceof TypeError, err.code);
}

try {
  tls.createSecureContext({ ciphers: 1 as any });
  console.log("invalid ciphers: no throw");
} catch (err: any) {
  console.log("invalid ciphers:", err instanceof TypeError, err.code);
}

try {
  tls.createSecureContext({ cert: "bad pem" });
  console.log("invalid cert pem: no throw");
} catch (err: any) {
  console.log("invalid cert pem:", err instanceof Error, err.code);
}
