import tls from "node:tls";

function probe(label: string, options: any) {
  try {
    tls.createSecureContext(options);
    console.log(label + ": ok");
  } catch (err: any) {
    console.log(label + ":", err instanceof TypeError, err.code);
  }
}
probe("default", {});
probe("tls13", { ciphers: "TLS_AES_128_GCM_SHA256" });
probe("empty", { ciphers: "" });
probe("number", { ciphers: 1 });
probe("unknown", { ciphers: "NOT_A_CIPHER" });
