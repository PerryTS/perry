import tls from "node:tls";

function probe(label: string, options: any) {
  try {
    const context = tls.createSecureContext(options);
    console.log(label + ":", context instanceof tls.SecureContext);
  } catch (err: any) {
    console.log(label + ":", err instanceof TypeError, err.code);
  }
}
probe("range", { minVersion: "TLSv1.2", maxVersion: "TLSv1.3" });
probe("invalid minimum", { minVersion: "TLSv9" });
probe("invalid maximum", { maxVersion: "TLSv9" });
probe("minimum conflict", { secureProtocol: "TLSv1_2_method", minVersion: "TLSv1.2" });
probe("maximum conflict", { secureProtocol: "TLSv1_2_method", maxVersion: "TLSv1.2" });
probe("invalid method", { secureProtocol: "not-a-method" });
