import tls from "node:tls";

const roots = tls.rootCertificates;
console.log("identity:", roots === tls.rootCertificates);
console.log("shape:", Array.isArray(roots), Object.isFrozen(roots), roots.length > 1);
console.log("unique:", roots.length === new Set(roots).size);
console.log("pem:", roots.every((cert) => cert.startsWith("-----BEGIN CERTIFICATE-----\n")));
for (const [label, mutate] of [
  ["element", () => { (roots as any)[0] = "changed"; }],
  ["sort", () => { (roots as any).sort(); }],
  ["property", () => { (tls as any).rootCertificates = []; }],
] as const) {
  try {
    mutate();
    console.log(label + ": no throw");
  } catch (err: any) {
    console.log(label + ":", err instanceof TypeError);
  }
}
