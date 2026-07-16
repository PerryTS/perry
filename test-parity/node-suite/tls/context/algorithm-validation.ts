import tls from "node:tls";

function probe(label: string, options: any) {
  try {
    console.log(label + ":", tls.createSecureContext(options) instanceof tls.SecureContext);
  } catch (err: any) {
    console.log(label + ":", err instanceof Error, err instanceof TypeError, err.code ?? "none");
  }
}
probe("valid sigalgs", { sigalgs: "rsa_pss_rsae_sha256" });
probe("sigalgs type", { sigalgs: 1 });
probe("sigalgs value", { sigalgs: "not-an-algorithm" });
probe("valid curve", { ecdhCurve: "prime256v1" });
probe("curve type", { ecdhCurve: 1 });
probe("curve value", { ecdhCurve: "not-a-curve" });
