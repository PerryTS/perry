import tls from "node:tls";

for (const value of [undefined, null, false, 0, ""] as any[]) {
  try {
    const context = tls.createSecureContext(value);
    console.log(typeof value + ":", context instanceof tls.SecureContext, Object.keys(context).join(","));
  } catch (err: any) {
    console.log(typeof value + ":", err.name, err.code);
  }
}
