import tls from "node:tls";

for (const [label, value] of [
  ["null", null],
  ["false", false],
  ["undefined", undefined],
  ["empty string", ""],
  ["zero", 0],
] as const) {
  try {
    const context = tls.createSecureContext({ key: value, cert: value, ca: value });
    console.log(label + ":", context instanceof tls.SecureContext);
  } catch (err: any) {
    console.log(label + ":", err.name, err.code);
  }
}
