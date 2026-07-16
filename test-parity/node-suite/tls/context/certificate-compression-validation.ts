import tls from "node:tls";

const getAlgorithms = (tls as any).getCertificateCompressionAlgorithms;
const supported: string[] = typeof getAlgorithms === "function"
  ? getAlgorithms()
  : [];
function probe(label: string, options: any) {
  try {
    console.log(
      label + ":",
      tls.createSecureContext(options) instanceof tls.SecureContext,
    );
  } catch (err: any) {
    console.log(label + ":", err instanceof TypeError, err.code ?? "none");
  }
}
probe("default", {});
probe("undefined", { certificateCompression: undefined });
probe("null", { certificateCompression: null });
probe("empty", { certificateCompression: [] });
probe("boolean", { certificateCompression: true });
probe("string", { certificateCompression: "zlib" });
probe("invalid", { certificateCompression: ["invalid"] });
probe("non string", { certificateCompression: [1] });
if (supported.length > 0) {
  probe("supported", { certificateCompression: supported });
  probe("tls12 conflict", {
    maxVersion: "TLSv1.2",
    certificateCompression: [supported[0]],
  });
}
