import tls from "node:tls";

function probe(label: string, options: any) {
  try {
    tls.createSecureContext(options);
    console.log(label + ": no throw");
  } catch (err: any) {
    console.log(label + ":", err instanceof TypeError, err.code);
  }
}
probe("key boolean", { key: true });
probe("cert boolean", { cert: true });
probe("ca object", { ca: {} });
probe("passphrase number", { key: "bad", passphrase: 1 });
probe("pfx object", { pfx: {} });
